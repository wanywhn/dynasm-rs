//! LoongArch registers and instruction representation
//! LoongArch has three main register families:
//! * General purpose registers (r0-r31 or specific names)
//! * Floating point registers (f0-f31)
//! * Vector registers for LSX/LASX (v0-v31/x0-x31)

use proc_macro2::Span;
use crate::common::Jump;
use super::loongarchdata::Opdata;
use std::fmt;

/// A generic register reference. Can be either a static RegId or a dynamic register from a family
#[derive(Debug, Clone)]
pub enum Register {
    Static(RegId),
    Dynamic(RegFamily, syn::Expr)
}

/// Unique identifiers for a specific register
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegId {
    // General purpose registers
    R0  = 0x00, R1  = 0x01, R2  = 0x02, R3  = 0x03, // zero, ra, tp, sp
    R4  = 0x04, R5  = 0x05, R6  = 0x06, R7  = 0x07, // a0, a1, a2, a3
    R8  = 0x08, R9  = 0x09, R10 = 0x0A, R11 = 0x0B, // a4, a5, a6, a7
    R12 = 0x0C, R13 = 0x0D, R14 = 0x0E, R15 = 0x0F, // t0, t1, t2, t3
    R16 = 0x10, R17 = 0x11, R18 = 0x12, R19 = 0x13, // t4, t5, t6, t7
    R20 = 0x14, R21 = 0x15, R22 = 0x16, R23 = 0x17, // t8, x, fp, s0
    R24 = 0x18, R25 = 0x19, R26 = 0x1A, R27 = 0x1B, // s1, s2, s3, s4
    R28 = 0x1C, R29 = 0x1D, R30 = 0x1E, R31 = 0x1F, // s5, s6, s7, s8

    // Floating point registers
    F0  = 0x20, F1  = 0x21, F2  = 0x22, F3  = 0x23,
    F4  = 0x24, F5  = 0x25, F6  = 0x26, F7  = 0x27,
    F8  = 0x28, F9  = 0x29, F10 = 0x2A, F11 = 0x2B,
    F12 = 0x2C, F13 = 0x2D, F14 = 0x2E, F15 = 0x2F,
    F16 = 0x30, F17 = 0x31, F18 = 0x32, F19 = 0x33,
    F20 = 0x34, F21 = 0x35, F22 = 0x36, F23 = 0x37,
    F24 = 0x38, F25 = 0x39, F26 = 0x3A, F27 = 0x3B,
    F28 = 0x3C, F29 = 0x3D, F30 = 0x3E, F31 = 0x3F,

    // Vector registers (LSX)
    V0  = 0x40, V1  = 0x41, V2  = 0x42, V3  = 0x43,
    V4  = 0x44, V5  = 0x45, V6  = 0x46, V7  = 0x47,
    V8  = 0x48, V9  = 0x49, V10 = 0x4A, V11 = 0x4B,
    V12 = 0x4C, V13 = 0x4D, V14 = 0x4E, V15 = 0x4F,
    V16 = 0x50, V17 = 0x51, V18 = 0x52, V19 = 0x53,
    V20 = 0x54, V21 = 0x55, V22 = 0x56, V23 = 0x57,
    V24 = 0x58, V25 = 0x59, V26 = 0x5A, V27 = 0x5B,
    V28 = 0x5C, V29 = 0x5D, V30 = 0x5E, V31 = 0x5F,

    // Vector registers (LASX, 256-bit)
    X0  = 0xA0, X1  = 0xA1, X2  = 0xA2, X3  = 0xA3,
    X4  = 0xA4, X5  = 0xA5, X6  = 0xA6, X7  = 0xA7,
    X8  = 0xA8, X9  = 0xA9, X10 = 0xAA, X11 = 0xAB,
    X12 = 0xAC, X13 = 0xAD, X14 = 0xAE, X15 = 0xAF,
    X16 = 0xB0, X17 = 0xB1, X18 = 0xB2, X19 = 0xB3,
    X20 = 0xB4, X21 = 0xB5, X22 = 0xB6, X23 = 0xB7,
    X24 = 0xB8, X25 = 0xB9, X26 = 0xBA, X27 = 0xBB,
    X28 = 0xBC, X29 = 0xBD, X30 = 0xBE, X31 = 0xBF,

    // CFR
    FCC0 = 0x60, FCC1 = 0x61, FCC2 = 0x62, FCC3 = 0x63,
    FCC4 = 0x64, FCC5 = 0x65, FCC6 = 0x66, FCC7 = 0x67,

    // FCSR
    FCSR0 = 0x80, FCSR1 = 0x81, FCSR2 = 0x82, FCSR3 = 0x83,
}

/// Register families
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegFamily {
    INTEGER = 0,
    FP = 1,
    LSX = 2,
    LASX = 5,
    FCC = 3,
    FCSR = 4,
}

impl RegId {
    /// Encode this RegId in a 5-bit value
    pub fn code(self) -> u8 {
        self as u8 & 0x1F
    }

    /// Returns the family of this RegId
    pub fn family(self) -> RegFamily {
        match self as u8 >> 5 {
            0 => RegFamily::INTEGER,
            1 => RegFamily::FP,
            2 => RegFamily::LSX,
            3 => RegFamily::FCC,
            4 => RegFamily::FCSR,
            5 => RegFamily::LASX,
            _ => unreachable!(),
        }
    }
}

impl fmt::Display for RegId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.family() {
            RegFamily::INTEGER => write!(f, "r{}", self.code()),
            RegFamily::FP => write!(f, "f{}", self.code()),
            RegFamily::LSX => write!(f, "v{}", self.code()),
            RegFamily::LASX => write!(f, "x{}", self.code()),
            RegFamily::FCC => write!(f, "fcc{}", self.code()),
            RegFamily::FCSR => write!(f, "fcsr{}", self.code()),
        }
    }
}

impl Register {
    /// Get the 5-bit code for this Register, if statically known.
    #[allow(dead_code)] // code() used only in compiler static branch, not called directly elsewhere
    pub fn code(&self) -> Option<u8> {
        match self {
            Register::Static(code) => Some(code.code()),
            Register::Dynamic(_, _) => None
        }
    }

    /// Returns the family that this Register is of
    pub fn family(&self) -> RegFamily {
        match self {
            Register::Static(code) => code.family(),
            Register::Dynamic(family, _) => *family
        }
    }

    /// Returns true if this Register is dynamic
    #[allow(dead_code)] // is_dynamic() used only in compiler dynamic branch detection
    pub fn is_dynamic(&self) -> bool {
        match self {
            Register::Static(_) => false,
            Register::Dynamic(_, _) => true
        }
    }

    /// Returns Some(RegId) if this Register is static
    pub fn as_id(&self) -> Option<RegId> {
        match self {
            Register::Static(id) => Some(*id),
            Register::Dynamic(_, _) => None
        }
    }
}

/// A LoongArch parsed instruction argument
#[derive(Debug)]
#[allow(dead_code)] // LabelReference variant unused until parser supports PC-relative refs (P0-1)
pub enum RawArg {
    // An immediate value or identifier
    Immediate {
        value: syn::Expr
    },
    // A label target
    JumpTarget {
        jump: Jump
    },
    // A register
    Register {
        span: Span,
        reg: Register
    },
    // A memory reference with optional offset
    Reference {
        span: Span,
        offset: Option<syn::Expr>,
        base: Register,
    },
    // A PC-relative reference
    LabelReference {
        span: Span,
        jump: Jump,
        base: Register
    },
}

/// The result of parsing a single instruction
#[derive(Debug)]
pub struct ParsedInstruction {
    pub name: String,
    pub span: Span,
    pub args: Vec<RawArg>
}

#[derive(Debug)]
pub enum FlatArg {
    Immediate {
        value: syn::Expr
    },
    JumpTarget {
        jump: Jump
    },
    Register {
        span: Span,
        reg: Register
    },
    Default
}

/// The result of finding a match for an instruction
#[derive(Debug)]
pub struct MatchData {
    pub data: &'static Opdata,
    pub args: Vec<FlatArg>
}