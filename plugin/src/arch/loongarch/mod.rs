// FIXME remove this when implementation is complete
#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unreachable_code)]

use syn::parse;
use proc_macro_error2::emit_error;

pub mod loongarchdata;
pub mod ast;
pub mod parser;
pub mod matching;
pub mod compiler;
pub mod debug;

use crate::State;
use crate::arch::{Stmt, Jump, Size};
use crate::arch::Arch;

#[cfg(feature = "dynasm_opmap")]
pub use debug::create_opmap;
#[cfg(feature = "dynasm_extract")]
pub use debug::extract_opmap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoongArchTarget {
    LA32,  // 32-bit LoongArch
    LA64   // 64-bit LoongArch
}

impl LoongArchTarget {
    pub fn is_64_bit(&self) -> bool {
        match self {
            LoongArchTarget::LA32 => false,
            LoongArchTarget::LA64 => true,
        }
    }

    pub fn is_32_bit(&self) -> bool {
        !self.is_64_bit()
    }
}

struct Context<'a, 'b: 'a> {
    pub state: &'a mut State<'b>,
    pub target: LoongArchTarget,
    pub features: loongarchdata::ExtensionFlags
}

#[derive(Clone, Debug, Default)]
pub struct ArchLoongArch64 {
    features: loongarchdata::ExtensionFlags
}

impl Arch for ArchLoongArch64 {
    fn set_features(&mut self, features: &[syn::Ident]) {
        self.features = parse_features(features);
    }

    fn handle_static_reloc(&self, stmts: &mut Vec<Stmt>, reloc: Jump, size: Size) {
        handle_static_reloc_inner(stmts, reloc, size);
    }

    fn default_align(&self) -> u8 {
        0
    }

    fn compile_instruction(&self, state: &mut State, input: parse::ParseStream) -> parse::Result<()> {
        let mut ctx = Context {
            state,
            target: LoongArchTarget::LA64,
            features: self.features
        };

        compile_instruction_inner(&mut ctx, input)
    }
}

#[derive(Clone, Debug, Default)]
pub struct ArchLoongArch32 {
    features: loongarchdata::ExtensionFlags
}

impl Arch for ArchLoongArch32 {
    fn set_features(&mut self, features: &[syn::Ident]) {
        self.features = parse_features(features);
    }

    fn handle_static_reloc(&self, stmts: &mut Vec<Stmt>, reloc: Jump, size: Size) {
        handle_static_reloc_inner(stmts, reloc, size);
    }

    fn default_align(&self) -> u8 {
        0
    }

    fn compile_instruction(&self, state: &mut State, input: parse::ParseStream) -> parse::Result<()> {
        let mut ctx = Context {
            state,
            target: LoongArchTarget::LA32,
            features: self.features
        };

        compile_instruction_inner(&mut ctx, input)
    }
}

fn compile_instruction_inner(ctx: &mut Context, input: parse::ParseStream) -> parse::Result<()> {
    let instruction = parser::parse_instruction(ctx, input)?;
    let span = instruction.span;

    let match_data = match matching::match_instruction(ctx, instruction) {
        Err(None) => return Ok(()),
        Err(Some(e)) => {
            emit_error!(span, e);
            return Ok(())
        }
        Ok(m) => m
    };

    match compiler::compile_instruction(ctx, match_data) {
        Err(None) => return Ok(()),
        Err(Some(e)) => {
            emit_error!(span, e);
            return Ok(())
        }
        Ok(()) => ()
    }

    Ok(())
}

fn handle_static_reloc_inner(stmts: &mut Vec<Stmt>, reloc: Jump, size: Size) {
    let span = reloc.span();

    // TODO: Define proper LoongArch relocations
    stmts.push(Stmt::Const(0, size));
    stmts.push(reloc.encode(size.in_bytes(), size.in_bytes(), &[]));
}

fn parse_features(features: &[syn::Ident]) -> loongarchdata::ExtensionFlags {
    // TODO: Implement LoongArch feature parsing
    loongarchdata::ExtensionFlags::default()
}