use super::Context;
use super::loongarchdata::{Template, Command, Relocation};
use super::ast::{MatchData, FlatArg, Register};

use syn::spanned::Spanned;
use quote::{quote, quote_spanned};
use proc_macro2::{TokenStream, Span};
use proc_macro_error2::emit_error;

use crate::parse_helpers::{as_signed_number};
use crate::common::{Stmt, Size, delimited, bitmask, bitmask64};

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
                    Command::R(offset) => offset,
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
                | Command::Next => (),
                _ => panic!("Invalid argument processor")
            },

            FlatArg::Immediate { ref value } => match *command {
                Command::UImm(bits, scaling) => {
                    let span = value.span();
                    let range: u32 = bitmask(bits);

                    let mut imm_encoder = ImmediateEncoder::new(value);
                    imm_encoder.gather_fields(data.data.commands, i + 1, &mut statics);

                    match imm_encoder.static_value {
                        Some(static_value) => {
                            static_range_check(static_value, 0, range, scaling, span)?;
                        },
                        None => {
                            let check = if scaling == 0 {
                                quote_spanned!{ span =>
                                    _dyn_imm > #range
                                }
                            } else {
                                let zeromask: u32 = bitmask(scaling);
                                quote_spanned!{ span =>
                                    _dyn_imm > #range || _dyn_imm & #zeromask != 0u32
                                }
                            };
                            imm_encoder.emit_dynamic(false, false, check, &mut dynamics);
                        }
                    }
                },
                Command::SImm(bits, scaling) => {
                    let span = value.span();
                    let range = bitmask(bits);
                    let min: i32 = (-1) << (bits - 1);

                    let mut imm_encoder = ImmediateEncoder::new(value);
                    imm_encoder.gather_fields(data.data.commands, i + 1, &mut statics);

                    match imm_encoder.static_value {
                        Some(static_value) => {
                            static_range_check(static_value, min, range, scaling, span)?;
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
                Command::BitRange(offset, bits, scaling) => (),
                Command::RBitRange(offset, bits, scaling) => (),
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
                                Command::BitRange(10, 16, 2),
                                Command::Next
                            ];
                        },
                        // 26-bit offset, 2-bit aligned
                        Relocation::J => {
                            bits = 26;
                            scaling = 2;
                            commands = &[
                                Command::BitRange(10, 16, 2),
                                Command::BitRange(26, 10, 18),
                                Command::Next
                            ];
                        },
                        // 32-bit PC-relative offset
                        Relocation::PC32 => {
                            bits = 32;
                            scaling = 0;
                            commands = &[
                                Command::RBitRange(10, 12, 2),
                                Command::BitRange(22, 20, 12),
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
                            static_range_check(static_value, min, range, scaling, span)?;
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
            Command::UImm(_, _)
            | Command::SImm(_, _)
            | Command::BitRange(_, _, _)
            | Command::RBitRange(_, _, _) => (),
            _ => cursor += 1
        }
    }

    // sanity
    if cursor != data.args.len() {
        panic!("Not enough command processors");
    }

    let mut templates = [0u32; 8];
    let mut exprs = [None, None, None, None, None, None, None, None];

    // for convenience sake we operate in 32 bits width
    match data.data.template {
        Template::Single(val) => templates[0] = val,
        Template::Double(val1, val2) => {
            templates[0] = val1;
            templates[1] = val2;
        },
        Template::Many(values) => {
            templates[..values.len()].copy_from_slice(values);
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
            for i in 0..2 {
                if let Some(d) = exprs[i].take() {
                    ctx.state.stmts.push(Stmt::ExprUnsigned(delimited(d), Size::B_4));
                } else {
                    ctx.state.stmts.push(Stmt::Const(u64::from(templates[i]), Size::B_4));
                }
            }
        },
        Template::Many(c) => {
            for i in 0..c.len() {
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

    pub fn gather_fields(&mut self, commands: &[Command], mut index: usize, statics: &mut Vec<(u8, u32)>) {
        loop {
            match commands.get(index) {
                Some(&Command::BitRange(offset, bits, scaling)) => {
                    let mask = bitmask(bits);

                    if let Some(v) = self.static_value {
                        let slice = (v >> scaling) as u32 & mask;
                        statics.push((offset, slice));
                    } else {
                        self.encodes.push((offset, quote_spanned!{ self.span=>
                            ((_dyn_imm >> #scaling) as u32 & #mask)
                        }));
                    }
                },
                Some(&Command::RBitRange(offset, bits, scaling)) => {
                    let mask = bitmask(bits);
                    let round_offset: i64 = 1 << (scaling - 1);

                    if let Some(v) = self.static_value {
                        let slice = (v.wrapping_add(round_offset) >> scaling) as u32 & mask;
                        statics.push((offset, slice));
                    } else {
                        let round_offset = proc_macro2::Literal::i64_unsuffixed(round_offset);
                        self.encodes.push((offset, quote_spanned!{ self.span=>
                            ((_dyn_imm.wrapping_add(#round_offset) >> #scaling) as u32 & #mask)
                        }));
                    }
                },
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
fn static_range_check(value: i64, min: i32, range: u32, scale: u8, span: Span) -> Result<u32, Option<String>> {
    if value < i64::from(min) {
        emit_error!(span, "Immediate too low");
        return Err(None);
    }

    let biased = value - i64::from(min);

    if biased > i64::from(range) {
        emit_error!(span, "Immediate too high");
        return Err(None);
    }

    let biased = biased as u32;

    if scale != 0 && (biased & bitmask(scale)) != 0 {
        emit_error!(span, "Unrepresentable immediate");
        return Err(None);
    }

    Ok(biased)
}