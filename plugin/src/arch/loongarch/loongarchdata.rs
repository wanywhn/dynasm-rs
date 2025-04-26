//! This file contains the datastructure specification for the LoongArch encoding data
use bitflags::bitflags;
use lazy_static::lazy_static;
use std::collections::{HashMap, hash_map};
use super::ast::RegId;
use std::fmt;

/// A template contains the information for the static parts of an instruction encoding
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Template {
    /// A single 32-bit instruction
    Single(u32),
    /// Two 32-bit instructions
    Double(u32, u32),
    /// A sequence of instructions
    Many(&'static [u32])
}

bitflags! {
    /// Flags indicating what ISA targets an instruction is valid on
    #[derive(Debug, Clone, Copy)]
    pub struct ISAFlags: u8 {
        const LA32 = 0x01;
        const LA64 = 0x02;
    }

    /// Flags specifying what ISA extensions are required for an instruction
    #[derive(Debug, Clone, Copy)]
    pub struct ExtensionFlags: u64 {
        /// Base integer instructions
        const Ex_BASE = 0x0000_0000_0000_0001;
        /// Bit manipulation instructions
        const Ex_BIT = 0x0000_0000_0000_0002;
        /// Single-precision floating-point
        const Ex_F = 0x0000_0000_0000_0004;
        /// Double-precision floating-point
        const Ex_D = 0x0000_0000_0000_0008;
        /// SIMD instructions (LSX)
        const Ex_LSX = 0x0000_0000_0000_0010;
        /// Advanced SIMD instructions (LASX)
        const Ex_LASX = 0x0000_0000_0000_0020;
        /// Virtual instructions (LVZ)
        const Ex_LVZ = 0x0000_0000_0000_0040;
        /// Binary translation instructions (LBT)
        const Ex_LBT = 0x0000_0000_0000_0080;
        /// Privileged instructions
        const Ex_PRIV = 0x0000_0000_0000_0100;
    }
}

impl ISAFlags {
    const fn make(bits: u8) -> ISAFlags {
        ISAFlags::from_bits_truncate(bits)
    }
}

impl ExtensionFlags {
    const fn make(bits: u64) -> ExtensionFlags {
        ExtensionFlags::from_bits_truncate(bits)
    }
}

impl fmt::Display for ExtensionFlags {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut first = true;
        for (flag, _) in self.iter_names() {
            if !first {
                write!(f, "_")?;
            }
            write!(f, "{}", &flag[3..])?;
            first = false;
        }
        Ok(())
    }
}

impl Default for ExtensionFlags {
    fn default() -> ExtensionFlags {
        ExtensionFlags::Ex_BASE
    }
}

/// Matchers validate the types of arguments passed to an instruction
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Matcher {
    /// A general purpose register
    R,
    /// A floating point register
    F,
    /// A specific register
    Reg(RegId),
    /// An indirect reference to a register
    Ref,
    /// An indirect reference with offset
    RefOffset,
    /// An immediate value
    Imm,
    /// A jump offset
    Offset,
    /// An identifier
    Ident,
}

/// Encoding commands specify how arguments should be encoded
#[derive(Debug, Clone)]
pub enum Command {
    // Meta commands
    /// Repeat the same argument again
    Repeat,
    /// Go to next argument
    Next,

    // Register fields
    /// A 5-bit register encoding
    R(u8),
    /// A 5-bit register encoding that cannot be r0
    Rno0(u8),
    /// A 5-bit floating point register encoding
    F(u8),

    // Immediate handling
    /// Unsigned immediate: bits, alignment
    UImm(u8, u8),
    /// Signed immediate: bits, alignment
    SImm(u8, u8),
    /// Jump offset
    Offset(Relocation),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relocation {
    // Branch instructions (beq, bne, etc)
    // 16-bit offset, 2-bit aligned
    B = 0,
    // Jump instructions (b, bl)
    // 26-bit offset, 2-bit aligned
    J = 1,
    // PC-relative load/store
    // 32-bit offset
    PC32 = 2,
    // 8-bit literal
    LITERAL8 = 3,
    // 16-bit literal
    LITERAL16 = 4,
    // 32-bit literal
    LITERAL32 = 5,
    // 64-bit literal
    LITERAL64 = 6,
}

impl Relocation {
    pub fn to_id(self) -> u8 {
        self as u8
    }

    pub fn size(self) -> u8 {
        match self {
            Relocation::LITERAL8 => 1,
            Relocation::LITERAL16 => 2,
            Relocation::B | Relocation::J | Relocation::PC32 | Relocation::LITERAL32 => 4,
            Relocation::LITERAL64 => 8,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Opdata {
    /// The base template for the encoding
    pub template: Template,
    /// What ISA targets this op is valid for
    pub isa_flags: ISAFlags,
    /// What extensions are required for this instruction
    pub ext_flags: &'static [ExtensionFlags],
    /// Matchers for instruction arguments
    pub matchers: &'static [Matcher],
    /// Encoder commands
    pub commands: &'static [Command],
}

macro_rules! SingleOp {
    ( $template:expr, $isa:expr, [ $( $matcher:expr ),* ], [ $( $command:expr ),* ], [ $( $extension:expr ),* ] ) => {
        {
            const MATCHERS: &'static [Matcher] = {
                #[allow(unused_imports)]
                use self::Matcher::*;
                &[ $(
                    $matcher
                ),* ]
            };
            const COMMANDS: &'static [Command] = {
                #[allow(unused_imports)]
                use self::Command::*;
                #[allow(unused_imports)]
                use self::Relocation::*;
                &[ $(
                    $command
                ),* ]
            };
            const EXTENSIONS: &'static [ExtensionFlags] = {
                #[allow(unused_imports)]
                &[ $(
                    ExtensionFlags::make($extension)
                ),* ]
            };

            use self::Template::*;
            Opdata {
                template: $template,
                isa_flags: ISAFlags::make($isa),
                ext_flags: EXTENSIONS,
                matchers: MATCHERS,
                commands: COMMANDS,
            }
        }
    }
}

macro_rules! Ops {
    ( $( $name:tt = [ $( $template:expr , $isa:expr , [ $( $matcher:expr ),* ] => [ $( $command:expr ),* ] , [ $( $extension:expr ),* ] ; )+ ] , )* ) => {
        [ $(
            (
                $name,
                &[ $(
                    SingleOp!( $template, $isa, [ $( $matcher ),* ], [ $( $command ),* ], [ $( $extension ),* ] )
                ),+ ] as &[_]
            )
        ),* ]
    }
}

pub fn get_mnemonic_data(name: &str) -> Option<&'static [Opdata]> {
    OPMAP.get(&name).cloned()
}

#[allow(dead_code)]
pub fn mnemonics() -> hash_map::Keys<'static, &'static str, &'static [Opdata]> {
    OPMAP.keys()
}

mod generated_data;
use generated_data::INSTRUCTIONS;

lazy_static! {
    static ref OPMAP: HashMap<&'static str, &'static [Opdata]> = {
        INSTRUCTIONS.iter().cloned().collect()
    };
}