use std::collections::HashMap;

use syn::{parse, Token};
use lazy_static::lazy_static;

use crate::parse_helpers::{parse_ident_or_rust_keyword, ParseOptExt};
use super::{Context, ast};

// Syntax for a single op: ident ("." ident)* (arg ("," arg)*)? ";"
pub(super) fn parse_instruction(ctx: &mut Context, input: parse::ParseStream) -> parse::Result<ast::ParsedInstruction> {
    let span = input.cursor().span();

    // Read the full dot-separated op
    let mut name = parse_ident_or_rust_keyword(input)?.to_string();
    // print!("{:#?}", input);
    while input.peek(Token![.]) {
        let _: Token![.] = input.parse()?;
        name.push('.'); 

        if input.peek(syn::LitInt) {
            let number: syn::LitInt = input.parse()?;
            name.push_str(number.base10_digits());
            name.push_str(number.suffix());
        } else {
            name.push_str(&parse_ident_or_rust_keyword(input)?.to_string());
        }

    }

    let mut args = Vec::new();

    // Parse 0 or more comma-separated args
    if !(input.is_empty() || input.peek(Token![;])) {
        args.push(parse_arg(ctx, input)?);

        while input.peek(Token![,]) {
            let _: Token![,] = input.parse()?;
            args.push(parse_arg(ctx, input)?);
        }
    }

    Ok(ast::ParsedInstruction {
        name,
        span,
        args
    })
}

/// Tries to parse a full arg definition
fn parse_arg(ctx: &mut Context, input: parse::ParseStream) -> parse::Result<ast::RawArg> {
    let start = input.cursor().span();

    // A label, identified by a leading < / > / -> / =>
    if let Some(jump) = input.parse_opt()? {
        return Ok(ast::RawArg::JumpTarget {
            jump
        });
    }

    // A memory reference. Format: offset[base] or [base]
    if input.peek(syn::token::Bracket) {
        let span = input.cursor().span();
        let inner;
        let _ = syn::bracketed!(inner in input);
        let inner = &inner;

        let base = parse_reg(ctx, inner)?.ok_or_else(|| inner.error("Expected register"))?;

        return Ok(ast::RawArg::Reference {
            span,
            base,
            offset: None
        });
    }

    // A register
    if let Some(reg) = parse_reg(ctx, input)? {
        return Ok(ast::RawArg::Register {
            reg,
            span: start
        })
    }

    // Immediate
    let expr: syn::Expr = input.parse()?;
    Ok(ast::RawArg::Immediate { value: expr })
}

/// Parses a single register, if present
/// This can be a simple register name (like `x5`)
/// an alias (any simple name that is registered, like `base`)
/// or a dynamic register (like `X(expr)`)
fn parse_reg(ctx: &mut Context, input: parse::ParseStream) -> parse::Result<Option<ast::Register>> {
    // Parse optional $ prefix
    // if input.peek(Token![$]) {
        // let _: Token![$] = input.parse()?;
    // }

    // We need to consume an ident, but only if it's one of the many we care about
    let name = input.step(|cursor| {
        if let Some((ident, rest)) = cursor.ident() {
            let mut ident = ident.to_string();

            // First, parse known register families
            if LOONGARCH_FAMILIES.contains_key(&*ident) {
                return Ok((ident, rest));
            }

            // Otherwise, see if this is an alias
            if let Some(repl) = ctx.state.invocation_context.aliases.get(&ident) {
                ident = repl.clone();
            }

            // Resolve normal register references
            if LOONGARCH_REGISTERS.contains_key(&*ident) {
                return Ok((ident, rest));
            }
        }
        Err(cursor.error("expected identifier"))
    });

    let name = match name {
        Ok(name) => name,
        Err(_) => return Ok(None)
    };

    // We know we have a register reference now, try to resolve it
    let register = if let Some(&id) = LOONGARCH_REGISTERS.get(&*name) {
        ast::Register::Static(id)
    } else if let Some(&family) = LOONGARCH_FAMILIES.get(&*name) {
        // Need to parse the trailing `( expr )`
        let inner;
        let _ = syn::parenthesized!(inner in input);
        let inner = &inner;

        let expr: syn::Expr = inner.parse()?;

        ast::Register::Dynamic(family, expr)
    } else {
        unreachable!()
    };

    Ok(Some(register))
}

lazy_static! {
    static ref LOONGARCH_REGISTERS: HashMap<&'static str, ast::RegId> = {
        use ast::RegId::*;

        static MAP: &[(&str, ast::RegId)] = &[
            // General purpose registers
            ("r0", R0), ("zero", R0),
            ("r1", R1), ("ra", R1),
            ("r2", R2), ("tp", R2),
            ("r3", R3), ("sp", R3),
            ("r4", R4), ("a0", R4),
            ("r5", R5), ("a1", R5),
            ("r6", R6), ("a2", R6),
            ("r7", R7), ("a3", R7),
            ("r8", R8), ("a4", R8),
            ("r9", R9), ("a5", R9),
            ("r10", R10), ("a6", R10),
            ("r11", R11), ("a7", R11),
            ("r12", R12), ("t0", R12),
            ("r13", R13), ("t1", R13),
            ("r14", R14), ("t2", R14),
            ("r15", R15), ("t3", R15),
            ("r16", R16), ("t4", R16),
            ("r17", R17), ("t5", R17),
            ("r18", R18), ("t6", R18),
            ("r19", R19), ("t7", R19),
            ("r20", R20), ("t8", R20),
            ("r21", R21),
            ("r22", R22), ("fp", R22),
            ("r23", R23), ("s0", R23),
            ("r24", R24), ("s1", R24),
            ("r25", R25), ("s2", R25),
            ("r26", R26), ("s3", R26),
            ("r27", R27), ("s4", R27),
            ("r28", R28), ("s5", R28),
            ("r29", R29), ("s6", R29),
            ("r30", R30), ("s7", R30),
            ("r31", R31), ("s8", R31),

            // Floating point registers
            ("f0", F0), ("f1", F1), ("f2", F2), ("f3", F3),
            ("f4", F4), ("f5", F5), ("f6", F6), ("f7", F7),
            ("f8", F8), ("f9", F9), ("f10", F10), ("f11", F11),
            ("f12", F12), ("f13", F13), ("f14", F14), ("f15", F15),
            ("f16", F16), ("f17", F17), ("f18", F18), ("f19", F19),
            ("f20", F20), ("f21", F21), ("f22", F22), ("f23", F23),
            ("f24", F24), ("f25", F25), ("f26", F26), ("f27", F27),
            ("f28", F28), ("f29", F29), ("f30", F30), ("f31", F31),

            // Vector registers
            ("v0", V0), ("v1", V1), ("v2", V2), ("v3", V3),
            ("v4", V4), ("v5", V5), ("v6", V6), ("v7", V7),
            ("v8", V8), ("v9", V9), ("v10", V10), ("v11", V11),
            ("v12", V12), ("v13", V13), ("v14", V14), ("v15", V15),
            ("v16", V16), ("v17", V17), ("v18", V18), ("v19", V19),
            ("v20", V20), ("v21", V21), ("v22", V22), ("v23", V23),
            ("v24", V24), ("v25", V25), ("v26", V26), ("v27", V27),
            ("v28", V28), ("v29", V29), ("v30", V30), ("v31", V31),

            // CFR
            ("fcc0", FCC0), ("fcc1", FCC1), ("fcc2", FCC2), ("fcc3", FCC3),
            ("fcc4", FCC4), ("fcc5", FCC5), ("fcc6", FCC6), ("fcc7", FCC7),

            // FCSR
            ("fcsr0", FCSR0), ("fcsr1", FCSR1), ("fcsr2", FCSR2), ("fcsr3", FCSR3),
        ];
        MAP.iter().cloned().collect()
    };

    static ref LOONGARCH_FAMILIES: HashMap<&'static str, ast::RegFamily> = {
        static MAP: &[(&str, ast::RegFamily)] = &[
            ("R", ast::RegFamily::INTEGER),
            ("F", ast::RegFamily::FP),
            ("V", ast::RegFamily::VECTOR)
        ];
        MAP.iter().cloned().collect()
    };
}