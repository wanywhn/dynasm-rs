
use super::Context;
use super::loongarchdata::{Command, Relocation, Template};
use super::ast::{MatchData, FlatArg, Register};

use syn::spanned::Spanned;
use quote::{quote, quote_spanned};
use proc_macro2::{TokenStream, Span};
use proc_macro_error2::emit_error;

use crate::parse_helpers::{as_signed_number, as_unsigned_number};
use crate::common::{Stmt, Size, delimited, bitmask, bitmask64};
use proc_macro2::Literal;

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

    // Set to true when LiW32/LiD64 directly emits instructions, bypassing the template loop.
    let mut direct_emit = false;

    for (i, command) in data.data.commands.iter().enumerate(){
        // meta commands — don't access args, just adjust cursor
        match *command {
            Command::Repeat => {
                cursor -= 1;
                continue;
            },
            Command::Next => {
                cursor += 1;
                continue;
            },
            Command::BitRange(_, _, _) | Command::RBitRange(_, _, _) => {
                // Encoding commands consumed by gather_fields; don't access args or advance cursor
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
                                    Relocation::PCADD_SHIFT2 => {
                                        let arr = &[5, 20];
                                        fun_name(&mut statics, &mut dynamics, value, arr, 2)?;
                                    },
                                    Relocation::PCADD_SHIFT12 => {
                                        let arr = &[5, 20];
                                        fun_name(&mut statics, &mut dynamics, value, arr, 12)?;
                                    },
                                    Relocation::PCADD_SHIFT18 => {
                                        let arr = &[5, 20];
                                        fun_name(&mut statics, &mut dynamics, value, arr, 18)?;
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
                                        fun_name(&mut statics, &mut dynamics, value, arr, 12)?;
                                    },
                                    Relocation::SPLIT_PCALA => {
                                        // SPLIT relocation: pcalau12i + addi.d pair (8 bytes)
                                        // hi20 with +0x800 rounding at bits [24:5] of first inst
                                        // lo12 at bits [21:10] of second inst (+32 bit offset)
                                        let static_val = as_signed_number(value);
                                        if let Some(sv) = static_val {
                                            // Static: compute hi20 with +0x800 compensation and lo12 directly
                                            let compensated = sv + 0x800;
                                            if (compensated >> 12) < -524288 || (compensated >> 12) > 524287 {
                                                emit_error!(value, "SPLIT_PCALA immediate out of range");
                                                return Err(None);
                                            }
                                            let hi20: u32 = ((compensated as i64 >> 12) as u32) & 0xF_FFFF;
                                            let lo12: u32 = (sv as u32) & 0xFFF;
                                            statics.push((5, hi20));      // first inst, bits [24:5]
                                            statics.push((42, lo12));     // second inst (10+32), bits [21:10]
                                        } else {
                                            // Dynamic: emit runtime computation
                                            let hi20_mask: u32 = 0xF_FFFF;
                                            let lo12_mask: u32 = 0xFFF;
                                            dynamics.push((5, quote_spanned!{ value.span()=>
                                                ((({let _v: i32 = #value; _v.wrapping_add(0x800)} as u32) >> 12) & #hi20_mask)
                                            }));
                                            dynamics.push((42, quote_spanned!{ value.span()=>
                                                ((#value as u32) & #lo12_mask)
                                            }));
                                        }
                                    },
                                    Relocation::SPLIT_CALL30 => {
                                        // pcaddu12i + jirl pair (8 bytes)
                                        // hi20 = val[31:12] at bits [24:5] of first inst (no compensation)
                                        // lo10 = val[11:2] at bits [25:10] of second inst
                                        let static_val = as_signed_number(value);
                                        if let Some(sv) = static_val {
                                            let hi20: u32 = ((sv as u32) >> 12) & 0xF_FFFF;
                                            let lo10: u32 = ((sv as u32) >> 2) & 0x3FF;
                                            statics.push((5, hi20));      // first inst, bits [24:5]
                                            statics.push((42, lo10));     // second inst (10+32), bits [25:10]
                                        } else {
                                            let hi20_mask: u32 = 0xF_FFFF;
                                            let lo10_mask: u32 = 0x3FF;
                                            dynamics.push((5, quote_spanned!{ value.span()=>
                                                ((#value as u32) >> 12) & #hi20_mask
                                            }));
                                            dynamics.push((42, quote_spanned!{ value.span()=>
                                                ((#value as u32) >> 2) & #lo10_mask
                                            }));
                                        }
                                    },
                                    Relocation::SPLIT_CALL36 => {
                                        // pcaddu18i + jirl pair (8 bytes)
                                        // hi20 = (val+0x20000)[37:18] at bits [24:5] of first inst (+0x20000 compensation)
                                        // lo16 = val[17:2] at bits [25:10] of second inst
                                        let static_val = as_signed_number(value);
                                        if let Some(sv) = static_val {
                                            let compensated = sv as i64 + 0x20000;
                                            if compensated >> 18 < -524288 || compensated >> 18 > 524287 {
                                                emit_error!(value, "SPLIT_CALL36 immediate out of range");
                                                return Err(None);
                                            }
                                            let hi20: u32 = ((compensated as u32) >> 18) & 0xF_FFFF;
                                            let lo16: u32 = ((sv as u32) >> 2) & 0xFFFF;
                                            statics.push((5, hi20));      // first inst, bits [24:5]
                                            statics.push((42, lo16));     // second inst (10+32), bits [25:10]
                                        } else {
                                            let hi20_mask: u32 = 0xF_FFFF;
                                            let lo16_mask: u32 = 0xFFFF;
                                            dynamics.push((5, quote_spanned!{ value.span()=>
                                                ((({let _v: i64 = #value as i64; _v.wrapping_add(0x20000)} as u32) >> 18) & #hi20_mask)
                                            }));
                                            dynamics.push((42, quote_spanned!{ value.span()=>
                                                ((#value as u32) >> 2) & #lo16_mask
                                            }));
                                        }
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

                // BigImm declares that the immediate is wider than 32 bits, enabling
                // BitRange/RBitRange to extract different bit ranges across multi-instruction templates.
                Command::BigImm(bits) => {
                    let span = value.span();
                    let range = bitmask64(bits);
                    let min: i64 = (-1) << (bits - 1);

                    let mut imm_encoder = ImmediateEncoder::new(value);
                    imm_encoder.gather_fields(data.data.commands, i + 1, &mut statics);

                    match imm_encoder.static_value {
                        Some(static_value) => {
                            if static_value < min {
                                emit_error!(span, "Immediate too low");
                                return Err(None);
                            }
                            if static_value.wrapping_sub(min) as u64 > range {
                                emit_error!(span, "Immediate too high");
                                return Err(None);
                            }
                        },
                        None => {
                            let check = quote_spanned!{ span =>
                                _dyn_imm.wrapping_sub(#min) as u64 > #range
                            };
                            imm_encoder.emit_dynamic(true, true, check, &mut dynamics);
                        }
                    }
                },

                // BitRange/RBitRange are encoding commands consumed by gather_fields
                // (handled as meta commands at loop top, never reached here)
                Command::BitRange(_, _, _) | Command::RBitRange(_, _, _) =>(),

                // li.w pseudo-instruction: adaptive instruction count for static values
                // LLVM generatesInstSeq: ori(1), addi.w(1), lu12i.w(1), or lu12i.w+ori(2)
                Command::LiW32 => {
                    let static_val = as_signed_number(value);
                    if let Some(sv) = static_val {
                        let v: i32 = sv as i32;
                        let lo12: u32 = (v as u32) & 0xFFF;
                        let hi20_se: i32 = v >> 12;  // sign-extended hi20

                        // Case 1: Hi20 == 0 → single ori $rd, $r0, Lo12
                        if hi20_se == 0 {
                            let rd = extract_rd_static(&data.args);
                            let insn = 0x03800000u32 | (rd as u32) | (lo12 << 10);
                            ctx.state.stmts.push(Stmt::Const(u64::from(insn), Size::B_4));
                            direct_emit = true;
                        }
                        // Case 2: lo12 == 0 → single lu12i.w $rd, Hi20
                        // (+0x800 compensation is automatic from sign extension semantics)
                        else if lo12 == 0 {
                            let rd = extract_rd_static(&data.args);
                            let si20: u32 = ((v + 0x800) >> 12) as u32 & 0xF_FFFF;
                            let insn = 0x14000000u32 | (rd as u32) | (si20 << 5);
                            ctx.state.stmts.push(Stmt::Const(u64::from(insn), Size::B_4));
                            direct_emit = true;
                        }
                        // Case 3: sign(Lo12) == sign(Hi20) → single addi.w $rd, $r0, Lo12
                        // This handles values like -1 (0xFFFFFFFF), 0xFFFFF000, etc.
                        // Condition: SignExtend32<1>(Lo12 >> 11) == SignExtend32<20>(Hi20)
                        // Simplified: ((lo12 >> 11) as i32 == hi20_se's sign bit)
                        else if ((lo12 >> 11) as i32) == (hi20_se >> 19) {
                            let rd = extract_rd_static(&data.args);
                            let si12: u32 = (v as u32) & 0xFFF;
                            let insn = 0x02800000u32 | (rd as u32) | (si12 << 10);
                            ctx.state.stmts.push(Stmt::Const(u64::from(insn), Size::B_4));
                            direct_emit = true;
                        }
                        // General case: lu12i.w + ori (2 instructions via template)
                        else {
                            let compensated = sv + 0x800;
                            if (compensated >> 12) < -524288 || (compensated >> 12) > 524287 {
                                emit_error!(value, "li.w immediate out of 32-bit range");
                                return Err(None);
                            }
                            let hi20: u32 = ((compensated as i64 >> 12) as u32) & 0xF_FFFF;
                            statics.push((5, hi20));      // first inst, bits [24:5]
                            statics.push((42, lo12));     // second inst (10+32), bits [21:10]
                        }
                    } else {
                        // Dynamic: always use 2-instruction template
                        let hi20_mask: u32 = 0xF_FFFF;
                        let lo12_mask: u32 = 0xFFF;
                        dynamics.push((5, quote_spanned!{ value.span()=>
                            ((({let _v: i32 = #value; _v.wrapping_add(0x800)} as u32) >> 12) & #hi20_mask)
                        }));
                        dynamics.push((42, quote_spanned!{ value.span()=>
                            ((#value as u32) & #lo12_mask)
                        }));
                    }
                },

                // li.d pseudo-instruction: adaptive instruction count for static values
                // LLVM generateInstSeq produces 1-4 instructions based on value analysis
                Command::LiD64 => {
                    let span = value.span();
                    let static_val = as_signed_number(value);
                    if let Some(sv) = static_val {
	                        let v: i64 = sv as i64;
	                        // LLVM's bit decomposition:
	                        // Highest12 = v[63:52], Higher20 = v[51:32], Hi20 = v[31:12], Lo12 = v[11:0]
	                        let highest12: u32 = ((v as u64) >> 52) as u32 & 0xFFF;
	                        let higher20: u32 = ((v as u64) >> 32) as u32 & 0xFFFFF;
	                        let hi20: u32 = ((v as u64) >> 12) as u32 & 0xFFFFF;
	                        let lo12: u32 = (v as u32) & 0xFFF;

	                        // LLVM's sign-extension checks:
	                        // SE1(x>>19) == SE20(y): if x sign bit=0 then y=0; if x sign bit=1 then y=0xFFFFF
	                        // SE1(x>>19) == SE12(y): if x sign bit=0 then y=0; if x sign bit=1 then y=0xFFF
	                        let skip_lu32i_d = sign_ext_1_to_20(hi20) == higher20;
	                        let skip_lu52i_d = sign_ext_1_to_12(higher20) == highest12;

	                        // Case 1: SE52(Val)==0 && Highest12!=0 -> single lu52i.d
	                        // SE52(Val)==0 means bits[51:0] sign-extend to zero, requiring them all to be 0.
	                        // With lo12==0, hi20==0, higher20==0, the lower 52 bits ARE all-zero.
	                        if highest12 != 0 && lo12 == 0 && hi20 == 0 && higher20 == 0 {
	                            let rd = extract_rd_static(&data.args);
	                            let si12: u32 = sign_extend_12_to_32(highest12) as u32 & 0xFFF;
	                            // lu52i.d $rd, $rd, si12 - rd starts as $r0 (value will be 0 before this inst)
	                            // lu52i.d modifies bits[63:52] of rd, leaving bits[51:0] unchanged.
	                            // If rd starts as 0, result is just si12 << 52. Correct for SE52==0 case.
	                            let insn = 0x03000000u32 | (rd as u32) | ((rd as u32) << 5) | (si12 << 10);
	                            ctx.state.stmts.push(Stmt::Const(u64::from(insn), Size::B_4));
	                            direct_emit = true;
	                        }
	                        // Case 2: hi32 is sign-extension of lo32 -> use li.w logic for lo32 only
	                        else if skip_lu32i_d && skip_lu52i_d {
	                            emit_li_w32_adaptive_static(ctx, v, &data.args);
	                            direct_emit = true;
	                        }
	                        // General case: emit lu12i.w + (ori if lo12!=0) + (lu32i.d if needed) + (lu52i.d if needed)
	                        else {
	                            let rd = extract_rd_static(&data.args);
	                            let rd_u32 = rd as u32;

	                            // lu12i.w $rd, hi20_0 (with +0x800 compensation)
	                            let compensated = v + 0x800;
	                            let si20_0: u32 = ((compensated >> 12) as u32) & 0xF_FFFF;
	                            let insn0 = 0x14000000u32 | rd_u32 | (si20_0 << 5);
	                            ctx.state.stmts.push(Stmt::Const(u64::from(insn0), Size::B_4));

	                            // ori $rd, $rd, lo12 (only if lo12 != 0)
	                            if lo12 != 0 {
	                                let insn1 = 0x03800000u32 | rd_u32 | (rd_u32 << 5) | (lo12 << 10);
	                                ctx.state.stmts.push(Stmt::Const(u64::from(insn1), Size::B_4));
	                            }

	                            // lu32i.d $rd, higher20 (only if not sign-extension of hi20)
	                            if !skip_lu32i_d {
	                                let si20_1: u32 = higher20 & 0xF_FFFF;
	                                let insn2 = 0x16000000u32 | rd_u32 | (si20_1 << 5);
	                                ctx.state.stmts.push(Stmt::Const(u64::from(insn2), Size::B_4));
	                            }

	                            // lu52i.d $rd, $rd, highest12 (only if not sign-extension of higher20)
	                            if !skip_lu52i_d {
	                                let si12_2: u32 = sign_extend_12_to_32(highest12) as u32 & 0xFFF;
	                                let insn3 = 0x03000000u32 | rd_u32 | (rd_u32 << 5) | (si12_2 << 10);
	                                ctx.state.stmts.push(Stmt::Const(u64::from(insn3), Size::B_4));
	                            }
	                            direct_emit = true;
	                        }
                    } else {
                        // Dynamic: always use 4-instruction template
                        let hi20_mask: u32 = 0xF_FFFF;
                        let lo12_mask: u32 = 0xFFF;
                        dynamics.push((5, quote_spanned!{ span =>
                            ((({let _v: i64 = #value as i64; _v.wrapping_add(0x800)} as u32) >> 12) & #hi20_mask)
                        }));
                        dynamics.push((42, quote_spanned!{ span =>
                            ((#value as u32) & #lo12_mask)
                        }));
                        dynamics.push((5+64, quote_spanned!{ span =>
                            (((#value as i64) >> 32) as u32 & #hi20_mask)
                        }));
                        dynamics.push((10+96, quote_spanned!{ span =>
                            (((#value as i64) >> 52) as u32 & #lo12_mask)
                        }));
                    }
                },
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
        // All non-meta commands advance cursor by 1 (each consumes one arg slot).
        // BitRange/RBitRange are encoding commands consumed by gather_fields (already handled above).
        cursor += 1;
    }

    // sanity
    if cursor != data.args.len() {
        panic!("Not enough command processors");
    }

    // If LiW32/LiD64 directly emitted instructions, skip the template loop
    if direct_emit {
        ctx.state.stmts.extend(relocations);
        return Ok(());
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
                    let round_offset: i64 =  1 << (scaling - 1);

                    if let Some(v) = self.static_value {
                        let slice = (v.wrapping_add(round_offset) >> scaling) as u32 & mask;
                        statics.push((offset, slice));

                    } else {
                        // ensure we emit an unsuffixed literal for this so it works with all
                        // types of number
                        let round_offset = Literal::i64_unsuffixed(round_offset);
                        self.encodes.push((offset, quote_spanned!{ self.span=>
                            ((_dyn_imm.wrapping_add(#round_offset) >> #scaling) as u32 & #mask)
                        }));
                    }
                },
                Some(Command::Next) => break,
                Some(_) | None => panic!("Bad encoding data, integer field sequence is not terminated"),
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

/// Extract the static rd register code from the first argument (assumed to be FlatArg::Register::Static).
/// Returns 0 for r0, 1 for r1, etc.
fn extract_rd_static(args: &[FlatArg]) -> u8 {
    match args.first() {
        Some(FlatArg::Register { reg: Register::Static(id), .. }) => id.code(),
        _ => panic!("li.w/li.d first argument must be a static register"),
    }
}

/// Sign-extend a 12-bit value to i32.
fn sign_extend_12_to_32(val: u32) -> i32 {
    ((val ^ (1 << 11)).wrapping_sub(1 << 11)) as i32
}

/// Sign-extend a 20-bit value to i32.
#[allow(dead_code)]
fn sign_extend_20_to_32(val: u32) -> i32 {
    ((val ^ (1 << 19)).wrapping_sub(1 << 19)) as i32
}

/// LLVM SE1(x>>19) == SE20(y) check: 1-bit sign extension must match 20-bit value.
/// If x's sign bit (bit 19) is 0, y must be 0; if x's sign bit is 1, y must be 0xFFFFF.
fn sign_ext_1_to_20(x: u32) -> u32 {
    if x & (1 << 19) != 0 { 0xFFFFF } else { 0 }
}

/// LLVM SE1(x>>19) == SE12(y) check: 1-bit sign extension must match 12-bit value.
/// If x's sign bit (bit 19) is 0, y must be 0; if x's sign bit is 1, y must be 0xFFF.
fn sign_ext_1_to_12(x: u32) -> u32 {
    if x & (1 << 19) != 0 { 0xFFF } else { 0 }
}

/// Emit the lo32 portion of a 64-bit immediate using li.w adaptive logic.
/// Produces 1 or 2 instructions (ori, addi.w, lu12i.w, or lu12i.w+ori).
/// The sign extension of lo32 will fill the hi32 bits automatically.
fn emit_li_w32_adaptive_static(ctx: &mut Context, v: i64, args: &[FlatArg]) {
    let rd = extract_rd_static(args);
    let rd_u32 = rd as u32;
    let lo32: i32 = v as i32;
    let lo12: u32 = (lo32 as u32) & 0xFFF;
    let hi20_se: i32 = lo32 >> 12;

    if hi20_se == 0 {
        // Case: val fits in 12 unsigned bits → single ori
        let insn = 0x03800000u32 | rd_u32 | (lo12 << 10);
        ctx.state.stmts.push(Stmt::Const(u64::from(insn), Size::B_4));
    } else if lo12 == 0 {
        // Case: lo12=0 → single lu12i.w (with +0x800 compensation)
        let si20: u32 = ((lo32 + 0x800) >> 12) as u32 & 0xF_FFFF;
        let insn = 0x14000000u32 | rd_u32 | (si20 << 5);
        ctx.state.stmts.push(Stmt::Const(u64::from(insn), Size::B_4));
    } else if ((lo12 >> 11) as i32) == (hi20_se >> 19) {
        // Case: sign(Lo12) == sign(Hi20) → single addi.w
        let si12: u32 = (lo32 as u32) & 0xFFF;
        let insn = 0x02800000u32 | rd_u32 | (si12 << 10);
        ctx.state.stmts.push(Stmt::Const(u64::from(insn), Size::B_4));
    } else {
        // General: lu12i.w + ori
        let compensated = lo32 + 0x800;
        let si20: u32 = ((compensated >> 12) as u32) & 0xF_FFFF;
        let insn0 = 0x14000000u32 | rd_u32 | (si20 << 5);
        let insn1 = 0x03800000u32 | rd_u32 | (rd_u32 << 5) | (lo12 << 10);
        ctx.state.stmts.push(Stmt::Const(u64::from(insn0), Size::B_4));
        ctx.state.stmts.push(Stmt::Const(u64::from(insn1), Size::B_4));
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
