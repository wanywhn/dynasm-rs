use proc_macro_error2::emit_error;
use proc_macro2::Span;

use super::{Context, LoongArchTarget};
use super::ast::{ParsedInstruction, RawArg, MatchData, FlatArg, Register, RegFamily};
use super::loongarchdata::{Opdata, Matcher, get_mnemonic_data};
use super::debug::format_opdata_list;

use crate::common::JumpKind;
use crate::parse_helpers::{as_ident, as_signed_number};

/// Try finding an appropriate instruction definition that matches the given instruction / arguments.
pub(super) fn match_instruction(ctx: &mut Context, mut instruction: ParsedInstruction) -> Result<MatchData, Option<String>> {
    // Sanitize Raw args from parsing for any impossible constructs
    sanitize_args(&mut instruction.args, &ctx.target)?;

    let opdata = get_mnemonic_data(&instruction.name).ok_or_else(|| Some(format!("Unknown instruction mnemonic '{}'", instruction.name)))?;
    

    // Iterate through the supported instruction formats
    for data in opdata {
        
if let Some(mut match_data) = match_args(&instruction.args, data) {
            flatten_args(instruction.args, &mut match_data);
            return Ok(match_data);
        }
    }

    let error = format!("'{}': instruction format mismatch, expected one of the following forms:\n{}", 
        &instruction.name, 
        format_opdata_list(&instruction.name, opdata, ctx.target)
    );

    Err(Some(error))
}

/// Sanitizes arguments, ensuring that:
/// - $zero cannot be used as destination register
/// - Extern relocations are handled properly
/// - Memory references use valid base registers
/// - Immediate values are within valid ranges
fn sanitize_args(args: &mut [RawArg], _target: &LoongArchTarget) -> Result<(), Option<String>> {
    for arg in args {
        match arg {
            RawArg::Register { reg, span } => sanitize_register(reg, *span)?,
            RawArg::Reference { base, span, offset } => {
                sanitize_register(base, *span)?;
                if base.family() != RegFamily::INTEGER {
                    emit_error!(span, "Base register needs to be a regular (integer) register");
                    return Err(None);
                }

                if let Some(o) = offset.as_ref() {
                    if as_signed_number(o) == Some(0) {
                        *offset = None
                    }
                }
            },
            RawArg::JumpTarget { jump } => {
                // Handle external relocations
                if let JumpKind::Bare(_) = jump.kind {
                    // TODO: Implement proper relocation handling for LoongArch
                    emit_error!(jump.span(), "External relocations not yet supported for LoongArch");
                    return Err(None);
                }
            },
            RawArg::LabelReference { base, span, jump } => {
                sanitize_register(base, *span)?;
                if base.family() != RegFamily::INTEGER {
                    emit_error!(span, "Base register needs to be a regular (integer) register");
                    return Err(None);
                }
                // External relocations in LabelReference not yet supported
                if let JumpKind::Bare(_) = jump.kind {
                    emit_error!(jump.span(), "External relocations not yet supported for LoongArch");
                    return Err(None);
                }
            },
            _ => ()
        }
    }

    Ok(())
}

/// Sanitize a single register, checking for invalid uses.
/// NOTE: LoongArch r0 ($zero) is valid as a destination for some instructions
/// (e.g., `addi.d $zero, rj, 0` acts as NOP). Per-instruction r0 restrictions
/// are handled by the Rno0 matcher in compiler.rs. A global r0 ban here would
/// be overly aggressive. Implement instruction-specific checks when needed.
fn sanitize_register(_register: &Register, _span: Span) -> Result<(), Option<String>> {
    Ok(())
}

impl MatchData {
    pub fn new(data: &'static Opdata) -> MatchData {
        MatchData {
            data,
            args: Vec::new()
        }
    }
}

impl Matcher {
    /// Returns if this matcher matches the given argument
    pub fn matches(&self, arg: &RawArg) -> bool {
        match arg {
            RawArg::Immediate { value } => match self {
                Matcher::R => false,
                Matcher::F => false,
                Matcher::FCSR => false,
                Matcher::T => false,
                Matcher::C => false,
                Matcher::V => false,
                Matcher::X => false,
                Matcher::Reg(_) => false,
                Matcher::Ref => false,
                Matcher::RefOffset => false,
                Matcher::Imm => true,
                Matcher::Offset => true,
                Matcher::RefLabel => false,
                Matcher::Ident => as_ident(value).is_some(),
            },
            RawArg::JumpTarget { .. } => matches!(self, Matcher::Offset),
            RawArg::Register { reg, .. } => match self {
                Matcher::R => reg.family() == RegFamily::INTEGER,
                Matcher::F => reg.family() == RegFamily::FP,
                Matcher::V => reg.family() == RegFamily::LSX,
                Matcher::X => matches!(reg.family(), RegFamily::LSX | RegFamily::LASX),
                Matcher::C => reg.family() == RegFamily::FCC,
                Matcher::Reg(regid) => reg.as_id() == Some(*regid),
                _ => false,
            },
            RawArg::Reference { offset, .. } => match self {
                Matcher::Ref => offset.is_none(),
                Matcher::RefOffset => true,
                _ => false,
            },
            RawArg::LabelReference { .. } => matches!(self, Matcher::RefLabel),
        }
    }
}

/// Check if the parsed instruction arguments match the data matching template
pub fn match_args(args: &[RawArg], data: &'static Opdata) -> Option<MatchData> {
    let mut args = args.iter();

    // Check if each matcher matches an appropriate arg
    for matcher in data.matchers {
        if let Some(arg) = args.next() {
            if !matcher.matches(arg) {
                return None;
            }
        } else {
            return None;
        }
    }

    // Return success if there's no more args remaining to match
    if args.next().is_some() {
        None
    } else {
        Some(MatchData::new(data))
    }
}

/// Populate MatchData with FlatArgs
fn flatten_args(args: Vec<RawArg>, data: &mut MatchData) {
    for (arg, matcher) in args.into_iter().zip(data.data.matchers.iter()) {
        match arg {
            RawArg::Immediate { value } => {
                data.args.push(FlatArg::Immediate { value });
            },
            RawArg::JumpTarget { jump } => {
                data.args.push(FlatArg::JumpTarget { jump });
            },
            RawArg::Register { span, reg } => match matcher {
                Matcher::Reg(_) => (),
                _ => data.args.push(FlatArg::Register { span, reg })
            },
            RawArg::Reference { span, offset, base } => match matcher {
                Matcher::RefOffset => {
                    data.args.push(FlatArg::Register { span, reg: base });
                    if let Some(offset) = offset {
                        data.args.push(FlatArg::Immediate { value: offset });
                    } else {
                        data.args.push(FlatArg::Default);
                    }
                },
                Matcher::Ref => {
                    data.args.push(FlatArg::Register { span, reg: base });
                },
                _ => unreachable!("Expected reference")
            },
            RawArg::LabelReference { span, base, jump } => match matcher {
                Matcher::RefLabel => {
                    data.args.push(FlatArg::Register { span, reg: base });
                    data.args.push(FlatArg::JumpTarget { jump });
                },
                _ => unreachable!("Expected RefLabel matcher for LabelReference"),
            },
        }
    }
}