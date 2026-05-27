
use super::Context;
use super::loongarchdata::{Command, Relocation, Template};
use super::ast::{MatchData, FlatArg, Register};

use syn::spanned::Spanned;
use quote::{quote, quote_spanned};
use proc_macro2::{TokenStream, Span};
use proc_macro_error2::emit_error;

use crate::parse_helpers::{as_signed_number, as_unsigned_number};
use crate::common::{Stmt, Size, delimited, bitmask};

/// Compile a single instruction. Input is taken from `data`, containing both the arguments
/// and the encoding template and commands.
/// Output is written to ctx.state
/// Errors can be emitted to either the whole instruction span (by returning Err(Some(errormsg)))
/// or emited specifically using emit_error! and returning Err(None)
pub(super) fn compile_instruction(ctx: &mut Context, data: MatchData) -> Result<(), Option<String>> {
    // argument cursor
    let mut cursor = 0usize;

    // All static bitfields (compile-time constant) will be encoded into this map of (offset, bitfield)
    let mut statics = Vec::new();
    // All dynamic bitfields (run-time determined) will be encoded into this map of (offset, TokenStream)
    let mut dynamics = Vec::new();
    // Any relocation will be encoded in this list
    let mut relocations = Vec::new();

    for (_i, command) in data.data.commands.iter().enumerate(){
        // meta commands
        match *command {
            Command::Repeat => {
                cursor -= 1;
                continue;
            },
            Command::Next => {
                cursor += 1;
                continue;
            },
            _ => ()
        }

        let arg = data.args.get(cursor).expect("Invalid encoding data, tried to process more arguments than given");

        match *arg {
            FlatArg::Register { span, reg: Register::Static(id) } => {
                let code = id.code();

                let offset = match *command {
                    Command::R(offset) | Command::F(offset) | Command::C(offset)
                    | Command::V(offset) | Command::X(offset) | Command::Rdiff(offset) => offset,
                    Command::Rno0(offset) => {
                        if code == 0 {
                            emit_error!(span, "This register must not be r0");
                            return Err(None);
                        }
                        offset
                    },
                    _ => panic!("Invalid argument processor, arg:{:?}, command:{:?}", arg, command)
                };

                statics.push((offset, u32::from(code)));
            },

            FlatArg::Register { span, reg: Register::Dynamic(_, ref expr) } => match *command {
                Command::R(offset) => {
                    dynamics.push((offset, quote_spanned!{ span=>
                        ((#expr & 0x1F) as u32)
                    }));
                },
                Command::Rno0(offset) => {
                    dynamics.push((offset, quote_spanned!{ span=>
                        {
                            let _dyn_reg: u8 = #expr;
                            if _dyn_reg == 0 {
                                ::dynasmrt::loongarch::invalid_register(_dyn_reg);
                            }
                            (_dyn_reg & 0x1F) as u32
                        }
                    }));
                },
                _ => panic!("Invalid argument processor, arg:{:?}, command:{:?}", arg, command)
            },

            FlatArg::Default => match *command {
                // Default is only emitted for a RefOffset where no offset was provided, i.e. it is 0
                Command::UImm(_, _)
                | Command::SImm(_, _)
                | Command::Ufields(_)
                | Command::Sfields(_)
                | Command::Next => (),
                _ => panic!("Invalid argument processor, arg:{:?}, command:{:?}", arg, command)
            },

            FlatArg::Immediate { ref value } => match *command {
                Command::UImm(offset, bitlen) => {
                                let span = value.span();
                                let mask = bitmask(bitlen);

                                if let Some((biased, _)) = static_range_check(value, 0, mask, 0, span)? {
                                    statics.push((offset, biased));

                                } else {
                                    let check = dynamic_range_check_unsigned(value.span(), 0, mask, 0);

                                    dynamics.push((offset, quote_spanned!{ value.span()=>
                                        { let _dyn_imm: u32 = #value; #check; _dyn_imm & #mask }
                                    }));
                                }
                            },
                Command::Sfields(arr) => {
                                fun_name(&mut statics, &mut dynamics, value, arr, 0)?;
                            },
Command::SImm(offset, bitlen) => {
                    // let arr = [offset, bitlen];
                    // fun_name(&mut statics, &mut dynamics, value, &arr, 0)?;
                    let mask = bitmask(bitlen);
                    let half = -1i32 << (bitlen - 1);
                    let span = value.span();
                    if let Some((_, scaled)) = static_range_check(value, half, mask, 0, span)? {
                        statics.push((offset, scaled & mask));
                    } else {
                        let check = dynamic_range_check_signed(value.span(), half, mask, 0);
                        dynamics.push((
                            offset,
                            quote_spanned! { value.span()=>
                                { let _dyn_imm: i32 = #value; #check; (_dyn_imm as u32) & #mask }
                            },
                        ));
                    }
                }
                    Command::Usum(offset, bitlen) => {
                        let mask = bitmask(bitlen);
                        let prev_value = if let Some(FlatArg::Immediate {value: prev_value } ) = data.args.get(cursor - 1) {
                            prev_value
                        } else {
                            panic!("Bad encoding data, previous argument was not an immediate");
                        };
    
                        let number = if let Some(prev_number) = as_unsigned_number(prev_value) {
                            if prev_number > mask as u64 {
                                emit_error!(prev_value, "Impossible immediate combination");
                                return Err(None);
                            };
    
                            if let Some((biased, _)) = static_range_check(value, 1, mask - (prev_number as u32), 0, value.span())? {
                                Some(biased + (prev_number as u32))
                            } else {
                                None
                            }
                        } else {
                            None
                        };
    
                        if let Some(number) = number {
                            statics.push((offset, number & mask));
                        } else {
                            let check = quote_spanned!{ value.span()=>
                                if (#value - 1u32) > (#mask - #prev_value) { ::dynasmrt::loongarch::immediate_out_of_range_unsigned_32(#value); }
                            };
    
                            dynamics.push((offset, quote_spanned!{ value.span()=>
                                { let _dyn_imm: u32 = #value; #check; (#prev_value + _dyn_imm - 1) & #mask }
                            }));
                        }
                    },
                    Command::Ulep(offset, bitlen) => {
                        let mask = bitmask(bitlen);
                        let prev_value = if let Some(FlatArg::Immediate {value: prev_value } ) = data.args.get(cursor - 1) {
                            prev_value
                        } else {
                            panic!("Bad encoding data, previous argument was not an immediate");
                        };
    
                        let number = if let Some(prev_number) = as_unsigned_number(prev_value) {
                            if prev_number > mask as u64 {
                                emit_error!(prev_value, "Impossible immediate combination");
                                return Err(None);
                            };
    
                            if let Some((biased, _)) = static_range_check(value, 0, prev_number as u32, 0, value.span())? {
                                Some(biased)
                            } else {
                                None
                            }
                        } else {
                            None
                        };
    
                        if let Some(number) = number {
                            statics.push((offset, number & mask));
                        } else {

                            let check = quote_spanned!{ value.span()=>
                                if (#value) > (#prev_value) { ::dynasmrt::loongarch::immediate_out_of_range_unsigned_32(#value); }
                            };
    
                            dynamics.push((offset, quote_spanned!{ value.span()=>
                                { let _dyn_imm: u32 = #value; #check; (_dyn_imm) & #mask }
                            }));
                        }
                    },
                Command::Offset(relocation_type) => {

                                // equivalent bitrange encodings for offsets
                                match relocation_type {
                                    Relocation::B16 => {
                                                                                            let arr = &[10, 16];
                                                                                            fun_name(&mut statics, &mut dynamics, value, arr, 2)?;
                                                                                        },
                                    Relocation::B26 => {
                                                                        let arr = &[0, 10, 10, 16];
                                                                        fun_name(&mut statics, &mut dynamics, value, arr, 2)?;
                                                                                        },
                                    Relocation::LITERAL8
                                                                                        | Relocation::LITERAL16
                                                                                        | Relocation::LITERAL32
                                                                                        | Relocation::LITERAL64 => panic!("Literal relocation in instruction"),
                                    Relocation::B21 => {
                                                                        let arr = &[0, 5, 10, 16];
                                                                        fun_name(&mut statics, &mut dynamics, value, arr, 2)?;
                                                                    },
                                    Relocation::ABS_HI20 => {
                                        let arr = &[5, 20];
                                        fun_name(&mut statics, &mut dynamics, value, arr, 0)?;
                                    },
                                    Relocation::SI14 => {
                                        let arr = &[10, 14];
                                        fun_name(&mut statics, &mut dynamics, value, arr, 2)?;
                                    },
                                    Relocation::SI12 | Relocation::PCALA_LO12 => {
                                        let arr = &[10, 12];
                                        fun_name(&mut statics, &mut dynamics, value, arr, 0)?;
                                    },
                                    Relocation::PCALA_HI20 => {
                                        // pcalau12i high 20 bits: bits [31:12] of offset, placed at [24:5]
                                        let arr = &[5, 20];
                                        fun_name(&mut statics, &mut dynamics, value, arr, 0)?;
                                    },
                                }

                            },
                Command::Uscaled(offset, len, scale) |
                Command::Sscaled(offset, len, scale) => {
                    let arr = &[offset, len];
                    fun_name(&mut statics, &mut dynamics, value, arr, scale)?;
                },
                Command::Usubone(offset, bitlen) => {
                    let mask = bitmask(bitlen);

                    if let Some((biased, _)) = static_range_check(value, 1, mask, 0, value.span())? {
                        statics.push((offset, biased));

                    } else {
                        let check = dynamic_range_check_unsigned(value.span(), 1, mask, 0);

                        dynamics.push((offset, quote_spanned!{ value.span()=>
                            { let _dyn_imm: u32 = #value; #check; (_dyn_imm - 1) & #mask }
                        }));
                    }
                },
                Command::Repeat |Command::Next | Command::R(_) | Command::Rdiff(_) |
                Command::Rno0(_) |Command::F(_) | Command::C(_) |
                Command::T(_) | Command::V(_) | Command::X(_) | Command::FCSR(_) |
                Command::Ufields(_) => panic!("Invalid argument processor, arg:{:?}, command:{:?}", arg, command),
            },

            FlatArg::JumpTarget { ref jump } => match *command {
                Command::Offset(relocation) => {
                    // encode the complete relocation. Always starts at the begin of the instruction(s), and also relative to that
                    let stmt = jump.clone().encode(relocation.size(), relocation.size(), &[relocation.to_id()]);
                    relocations.push(stmt);
                },
                _ => panic!("Invalid argument processor, arg:{:?}, command:{:?}", arg, command)
            }
        }

        // figure out how far the cursor has to be advanced.
        match *command {
            _ => cursor += 1
        }
    }

    // sanity
    if cursor != data.args.len() {
        panic!("Not enough command processors");
    }

    let mut templates = [0u32; 8];
    let mut exprs = [None, None, None, None, None, None, None, None];

    // for convenience sake we operate in 32 bits width, even for compressed instructions
    match data.data.template {
        Template::Single(val) => templates[0] = val,
        Template::Double(val1, val2) => {
            templates[0] = val1;
            templates[1] = val2;
        },
        Template::Many(values) => {
            templates[ .. values.len()].copy_from_slice(values);
        }
    };

    // apply all statics to templates
    for (offset, value) in statics {
        templates[(offset >> 5) as usize] |= value << (offset & 0x1F);
    }

    // and process all dynamics
    for (offset, expr) in dynamics {
        let index = usize::from(offset >> 5);
        let offset = offset & 0x1F;

        exprs[index] = match exprs[index].take() {
            Some(prev_expr) => {
                Some(if offset == 0 {
                    quote!{ #prev_expr | #expr }
                } else {
                    quote!{ #prev_expr | (#expr << #offset) }
                })
            },
            None => {
                let bits = templates[index];
                Some(if offset == 0 {
                    quote!{ #bits | #expr }
                } else {
                    quote!{ #bits | (#expr << #offset) }
                })
            }
        }
    }

    match data.data.template {
        Template::Single(_) => if let Some(d) = exprs[0].take() {
            ctx.state.stmts.push(Stmt::ExprUnsigned(delimited(d), Size::B_4));
        } else {
            ctx.state.stmts.push(Stmt::Const(u64::from(templates[0]), Size::B_4));
        },
        Template::Double(_, _) => {
            for i in 0 .. 2 {
                if let Some(d) = exprs[i].take() {
                    ctx.state.stmts.push(Stmt::ExprUnsigned(delimited(d), Size::B_4));
                } else {
                    ctx.state.stmts.push(Stmt::Const(u64::from(templates[i]), Size::B_4));
                }
            }
        },
        Template::Many(c) => {
            for i in 0 .. c.len() {
                if let Some(d) = exprs[i].take() {
                    ctx.state.stmts.push(Stmt::ExprUnsigned(delimited(d), Size::B_4));
                } else {
                    ctx.state.stmts.push(Stmt::Const(u64::from(templates[i]), Size::B_4));
                }
            }
        }
    }
    
    ctx.state.stmts.extend(relocations);

    Ok(())
}

fn fun_name(statics: &mut Vec<(u8, u32)>, dynamics: &mut Vec<(u8, TokenStream)>, value: &syn::Expr, arr: &[u8], scale :u8) -> Result<(), Option<String>> {
    let bitlen = arr.iter().enumerate()
    .filter(|(i, _)| i % 2 != 0)
    .map(|(_, &val)| val)
    .sum();
    let mask = bitmask(bitlen);
    let half = -1i32 << (bitlen - 1);
    let span = value.span();
    Ok(if let Some((_, scaled)) = static_range_check(value, half, mask, scale, span)? {
        let mut consumed_len = 0;
        for w in arr.windows(2).step_by(2) {
            let offset = w[0];
            let len = w[1];
            consumed_len += len;
            statics.push((offset, (scaled >> (bitlen - consumed_len) & bitmask(len))));
            // dynamics.push((offset + 1, quote_spanned!{ span=>
            //     (value >> #len) as u32
            // }));
        }

    } else {
        let check = dynamic_range_check_signed(value.span(), half, mask, scale);
        let mut consumed_len = 0;
        for w in arr.windows(2).step_by(2) {
            let offset = w[0];
            let len = w[1];
            let par_ask = bitmask(len);
            consumed_len += len;
            dynamics.push((offset, quote_spanned!{ value.span()=>
                {let _dyn_imm: i32 = #value; #check; ((_dyn_imm >> (#bitlen - #consumed_len)) as u32) & #par_ask }
            }));
        }
    })
}

/// Handles the encoding of immediates in a somewhat efficient fashion.
#[allow(dead_code)] // ImmediateEncoder unused until multi-instruction immediate encoding is fully implemented
struct ImmediateEncoder<'a> {
    pub dynamic_value: &'a syn::Expr,
    pub static_value: Option<i64>,
    pub encodes: Vec<(u8, TokenStream)>, // encoding_offset, expression
    pub span: Span
}

#[allow(dead_code)] // ImmediateEncoder methods unused until multi-instruction immediate encoding is fully implemented
impl<'a> ImmediateEncoder<'a> {
    pub fn new(dynamic_value: &'a syn::Expr) -> ImmediateEncoder<'a> {
        #![allow(unexpected_cfgs)]
        let static_value;

        // this allows turning off static checks for testing purposes
        #[cfg(not(disable_static_checks="1"))]
        {
            static_value = as_signed_number(dynamic_value);
        }
        #[cfg(disable_static_checks="1")]
        {
            static_value = None;
        }

        let span = dynamic_value.span();

        ImmediateEncoder {
            dynamic_value,
            static_value,
            encodes: Vec::new(),
            span
        }
    }

    #[allow(dead_code)] // gather_fields unused until multi-instruction immediate encoding is fully implemented
    pub fn gather_fields(&mut self, commands: &[Command], index: usize, _statics: &mut Vec<(u8, u32)>) {
        loop {
            match commands.get(index) {
                Some(Command::Next) => break,
                Some(_) | None => panic!("Bad encoding data, integer field sequence is not terminated"),
            }
        }
    }

    pub fn emit_dynamic(mut self, is_signed: bool, is_64bit: bool, check: TokenStream, dynamics: &mut Vec<(u8, TokenStream)>) {
        // assemble encoding chunks
        let mut exprs = [None, None, None, None, None, None, None, None];
        let dynamic_value = self.dynamic_value;
        let span = self.span;

        for (offset, expr) in self.encodes.drain(..) {
            let index = usize::from(offset >> 5);
            let offset = offset & 0x1F;

            exprs[index] = match exprs[index].take() {
                Some(prev_expr) => {
                    let parenthesized = delimited(prev_expr);

                    Some(if offset == 0 {
                        let expr = delimited(expr);
                        quote!{ #parenthesized | #expr }
                    } else {
                        quote!{ #parenthesized | (#expr << #offset) }
                    })
                },
                None => {
                    Some(if offset == 0 {
                        quote!{ #expr }
                    } else {
                        quote!{ #expr << #offset }
                    })
                }
            }
        }

        let imm_ty = match (is_64bit, is_signed) {
            (false, false) => quote_spanned!{ span=> u32 },
            (false, true)  => quote_spanned!{ span=> i32 },
            (true, false)  => quote_spanned!{ span=> u64 },
            (true, true)   => quote_spanned!{ span=> i64 }
        };

        let mut first = true;
        for (i, expr) in exprs.into_iter().enumerate() {
            let encodes = if let Some(encodes) = expr { encodes } else {
                continue
            };
            let offset = (i * 32) as u8;

            if first {
                first = false;
                let error_expr = match (is_64bit, is_signed) {
                    (false, false) => quote_spanned!{ span=>
                        ::dynasmrt::loongarch::immediate_out_of_range_unsigned_32
                    },
                    (false, true)  => quote_spanned!{ span=>
                        ::dynasmrt::loongarch::immediate_out_of_range_signed_32
                    },
                    (true, false)  => quote_spanned!{ span=>
                        ::dynasmrt::loongarch::immediate_out_of_range_unsigned_64
                    },
                    (true, true)   => quote_spanned!{ span=>
                        ::dynasmrt::loongarch::immediate_out_of_range_signed_64
                    }
                };

                dynamics.push((offset, quote_spanned!{ span=>
                    {
                        let _dyn_imm: #imm_ty = #dynamic_value;

                        if #check {
                            #error_expr(_dyn_imm);
                        }

                        #encodes
                    }
                }));
            } else {
                dynamics.push((offset, quote_spanned!{ span=>
                    {
                        let _dyn_imm: #imm_ty = #dynamic_value;
                        #encodes
                    }
                }));
            }
        }
    }
}

/// Checks the following things
/// value >= min
/// (value - min) <= range
/// ((value - min) & bitmask(scale)) == 0
/// returning (value - min) on success.
fn static_range_check(expr: &syn::Expr, min: i32, range: u32, scale: u8, _span: Span) -> Result<Option<(u32, u32)>, Option<String>> {
    #![allow(unexpected_cfgs)]

    // signed 64-bit parse is always safe for 32-bit numbers
    let value = match as_signed_number(expr) {
        Some(v) => v,
        None => return Ok(None)
    };

    // this allows turning off static checks for testing purposes
    #[cfg(disable_static_checks="1")]
    return Ok(None);


    // arithmetic right shift
    let scaled: i64 = value >> scale;
    if scaled << scale != value {
        emit_error!(expr, "Unrepresentable immediate scaled:{} scale:{} value:{} min:{} range:{}",
        scaled, scale, value, min, range);
        return Err(None);
    }

    let biased = scaled - i64::from(min);
    if biased < 0 {
        emit_error!(expr, "Immediate too low");
        Err(None)
    } else if biased > i64::from(range) {
        emit_error!(expr, "Immediate too high");
        Err(None)
    } else {
        // this cast is always safe
        Ok(Some((biased as u32, scaled as u32)))

    }
}

/// emits the code for a range check on an unsigned immediate.
fn dynamic_range_check_unsigned(span: Span, bias: u32, range: u32, scale: u8) -> TokenStream {
    let check = if scale == 0 {
        if bias == 0 {
            quote_spanned!{ span=> _dyn_imm > #range }
        } else {
            quote_spanned!{ span=> _dyn_imm.wrapping_sub(#bias) > #range }
        }
    } else {
        let mask = bitmask(scale);

        if bias == 0 {
            quote_spanned!{ span=> ((_dyn_imm & #mask) != 0) || (_dyn_imm >> #scale) > #range }
        } else {
            quote_spanned!{ span=> ((_dyn_imm & #mask) != 0) || (_dyn_imm >> #scale).wrapping_sub(#bias) > #range }
        }
    };

    quote_spanned!{ span => if #check { ::dynasmrt::loongarch::immediate_out_of_range_unsigned_32(_dyn_imm); }}
}

/// emits the code for a range check on a signed immediate.
fn dynamic_range_check_signed(span: Span, bias: i32, range: u32, scale: u8) -> TokenStream {
    let bias = -bias;

    let check = if scale == 0 {
        if bias == 0 {
            quote_spanned!{ span => (_dyn_imm as u32) > #range }
        } else {
            quote_spanned!{ span => (_dyn_imm.wrapping_add(#bias) as u32) > #range }
        }
    } else {
        let mask = bitmask(scale) as i32;

        if bias == 0 {
            quote_spanned!{ span=> ((_dyn_imm & #mask) != 0) || ((_dyn_imm >> #scale) as u32) > #range }
        } else {
            quote_spanned!{ span=> ((_dyn_imm & #mask) != 0) || ((_dyn_imm >> #scale).wrapping_add(#bias) as u32) > #range }
        }
    };

    quote_spanned!{ span => if #check { ::dynasmrt::loongarch::immediate_out_of_range_signed_32(_dyn_imm); }}
}
