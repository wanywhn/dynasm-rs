use super::Context;
use super::loongarchdata::{Command, Relocation};
use super::ast::{MatchData, FlatArg, Register};

use syn::spanned::Spanned;
use quote::{quote, quote_spanned};
use proc_macro2::{TokenStream, Span};
use proc_macro_error2::emit_error;

use crate::parse_helpers::{as_signed_number};
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

    for (i, command) in data.data.commands.iter().enumerate() {
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
                    Command::R(offset) | Command::F(offset) | Command::V(offset) | Command::X(offset)=> offset,
                    Command::Rno0(offset) => {
                        if code == 0 {
                            emit_error!(span, "This register must not be r0");
                            return Err(None);
                        }
                        offset
                    },
                    _ => panic!("Invalid argument processor")
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
                _ => panic!("Invalid argument processor")
            },

            FlatArg::Default => match *command {
                // Default is only emitted for a RefOffset where no offset was provided, i.e. it is 0
                Command::UImm(_, _)
                | Command::SImm(_, _)
                | Command::Ufields(_)
                | Command::Sfields(_)
                | Command::Next => (),
                _ => panic!("Invalid argument processor")
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
                    let bitlen = arr.iter().enumerate()
                    .filter(|(i, _)| i % 2 != 0)
                    .map(|(_, &val)| val)
                    .sum();

                    let mask = bitmask(bitlen);
                    let half = -1i32 << (bitlen - 1);
                    let span = value.span();

                    if let Some((biased, _)) = static_range_check(value, half, mask, 0, span)? {

                        for w in arr.windows(2).step_by(2) {
                            let offset = w[0];
                            let len = w[1];
                            statics.push((offset, (biased >> (bitlen - len) & bitmask(len))));
                            // dynamics.push((offset + 1, quote_spanned!{ span=>
                            //     (value >> #len) as u32
                            // }));
                        }

                    } else {
                        let check = dynamic_range_check_signed(value.span(), half, mask, 0);

                        for w in arr.windows(2).step_by(2) {
                            let offset = w[0];
                            let len = w[1];
                            let par_ask = bitmask(len);
                            dynamics.push((offset, quote_spanned!{ value.span()=>
                                {let _dyn_imm: i32 = #value; #check; ((value >> (#bitlen - #len)) as u32) & #par_ask }
                            }));
                        }
                    }
                },
                // signed immediate encoding

                Command::SImm(offset, bitlen) => {
                    let mask = bitmask(bitlen);
                    let half = -1i32 << (bitlen - 1);
                    let span = value.span();

                    if let Some((_, scaled)) = static_range_check(value, half, mask, 0, span)? {
                        statics.push((offset, scaled & mask));

                    } else {
                        let check = dynamic_range_check_signed(value.span(), half, mask, 0);

                        dynamics.push((offset, quote_spanned!{ value.span()=>
                            { let _dyn_imm: i32 = #value; #check; (_dyn_imm as u32) & #mask }
                        }));
                    }
                },
                Command::Offset(relocation_type) => {
                    let bits;
                    let scaling;
                    let commands: &'static [Command];

                    // equivalent bitrange encodings for offsets
                    match relocation_type {
                        // 16-bit offset, 2-bit aligned
                        Relocation::B => {
                            bits = 16;
                            scaling = 2;
                            commands = &[
                                // Command::BitRange(10, 16, 2),
                                Command::Next
                            ];
                        },
                        // 26-bit offset, 2-bit aligned
                        Relocation::J => {
                            bits = 26;
                            scaling = 2;
                            commands = &[
                                Command::Next
                            ];
                        },
                        // 32-bit PC-relative offset
                        Relocation::PC32 => {
                            bits = 32;
                            scaling = 0;
                            commands = &[
                                Command::Next
                            ];
                        },
                        Relocation::LITERAL8
                        | Relocation::LITERAL16
                        | Relocation::LITERAL32
                        | Relocation::LITERAL64 => panic!("Literal relocation in instruction"),
                    }

                    let span = value.span();
                    let range = bitmask(bits);
                    let min: i32 = (-1) << (bits - 1);

                    let mut imm_encoder = ImmediateEncoder::new(value);
                    imm_encoder.gather_fields(commands, 0, &mut statics);

                    match imm_encoder.static_value {
                        Some(static_value) => {
                            static_range_check(value, min, range, scaling, span)?;
                        },
                        None => {
                            let check = if scaling == 0 {
                                quote_spanned!{ span =>
                                    _dyn_imm.wrapping_sub(#min) as u32 > #range
                                }
                            } else {
                                let zeromask = bitmask(scaling) as i32;
                                quote_spanned!{ span =>
                                    _dyn_imm.wrapping_sub(#min) as u32 > #range || _dyn_imm & #zeromask != 0i32
                                }
                            };

                            imm_encoder.emit_dynamic(true, false, check, &mut dynamics);
                        }
                    }
                },
                _ => panic!("Invalid argument processor")
            },

            FlatArg::JumpTarget { ref jump } => match *command {
                Command::Offset(relocation) => {
                    // encode the complete relocation. Always starts at the begin of the instruction(s), and also relative to that
                    let stmt = jump.clone().encode(relocation.size(), relocation.size(), &[relocation.to_id()]);
                    relocations.push(stmt);
                },
                _ => panic!("Invalid argument processor")
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

    // apply all statics to bits
    let mut bits = data.data.template;
    for (offset, value) in statics {
        bits |= value << offset;
    }

    // generate code to be emitted for dynamics
    if !dynamics.is_empty() {
        let mut res = quote!{
            #bits
        };
        for (offset, expr) in dynamics {
            res = quote!{
                #res | ((#expr) << #offset)
            };
        }
        ctx.state.stmts.push(Stmt::ExprUnsigned(delimited(res), Size::B_4));
    } else {
        ctx.state.stmts.push(Stmt::Const(u64::from(bits), Size::B_4));
    }
    ctx.state.stmts.extend(relocations);

    Ok(())
}

/// Handles the encoding of immediates in a somewhat efficient fashion.
struct ImmediateEncoder<'a> {
    pub dynamic_value: &'a syn::Expr,
    pub static_value: Option<i64>,
    pub encodes: Vec<(u8, TokenStream)>, // encoding_offset, expression
    pub span: Span
}

impl<'a> ImmediateEncoder<'a> {
    pub fn new(dynamic_value: &syn::Expr) -> ImmediateEncoder {
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

    pub fn gather_fields(&mut self, commands: &[Command], index: usize, statics: &mut Vec<(u8, u32)>) {
        loop {
            match commands.get(index) {
                Some(Command::Next) => break,
                Some(_)
                | None => panic!("Bad encoding data, integer field sequence is not terminated"),
            }
            index += 1;
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
fn static_range_check(expr: &syn::Expr, min: i32, range: u32, scale: u8, span: Span) -> Result<Option<(u32, u32)>, Option<String>> {
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
        emit_error!(expr, "Unrepresentable immediate");
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

    quote_spanned!{ span => if #check { ::dynasmrt::aarch64::immediate_out_of_range_unsigned_32(_dyn_imm); }}
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

    quote_spanned!{ span => if #check { ::dynasmrt::aarch64::immediate_out_of_range_signed_32(_dyn_imm); }}
}
