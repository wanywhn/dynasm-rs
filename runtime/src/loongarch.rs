
//! Runtime support for the LoongArch architecture assembling target.

use crate::Register;
use crate::relocations::{Relocation, RelocationSize, RelocationKind, ImpossibleRelocation};

/// Relocation implementation for the LoongArch architecture.
#[derive(Debug, Clone)]
#[allow(missing_docs)]
pub enum LoongArchRelocation {
    // TODO: Define LoongArch-specific relocation types
    Plain(RelocationSize),
}

impl LoongArchRelocation {
    fn op_mask(&self) -> u32 {
        match self {
            Self::Plain(_) => 0
        }
    }
}

impl Relocation for LoongArchRelocation {
    type Encoding = (u8,);
    fn from_encoding(encoding: Self::Encoding) -> Self {
        Self::Plain(RelocationSize::from_encoding(encoding.0))
    }
    fn from_size(size: RelocationSize) -> Self {
        Self::Plain(size)
    }
    fn size(&self) -> usize {
        match self {
            Self::Plain(s) => s.size(),
        }
    }
    fn write_value(&self, buf: &mut [u8], value: isize) -> Result<(), ImpossibleRelocation> {
        if let Self::Plain(s) = self {
            return s.write_value(buf, value);
        };
        Ok(())
    }
    fn read_value(&self, buf: &[u8]) -> isize {
        if let Self::Plain(s) = self {
            return s.read_value(buf);
        };
        0
    }
    fn kind(&self) -> RelocationKind {
        RelocationKind::Relative
    }
    fn page_size() -> usize {
        4096
    }
}

/// A LoongArch Assembler
pub type Assembler = crate::Assembler<LoongArchRelocation>;
/// A LoongArch AssemblyModifier
pub type AssemblyModifier<'a> = crate::Modifier<'a, LoongArchRelocation>;
/// A LoongArch UncommittedModifier
pub type UncommittedModifier<'a> = crate::UncommittedModifier<'a>;

// TODO: Define LoongArch register enums
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum R {
    R0 = 0, R1 = 1, R2 = 2, R3 = 3,
    R4 = 4, R5 = 5, R6 = 6, R7 = 7,
    R8 = 8, R9 = 9, R10 = 10, R11 = 11,
    R12 = 12, R13 = 13, R14 = 14, R15 = 15,
    R16 = 16, R17 = 17, R18 = 18, R19 = 19,
    R20 = 20, R21 = 21, R22 = 22, R23 = 23,
    R24 = 24, R25 = 25, R26 = 26, R27 = 27,
    R28 = 28, R29 = 29, R30 = 30, R31 = 31,
}
reg_impls!(R);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_assembler_creation() {
        // Basic test to verify assembler can be created
        let _ = Assembler::new();
    }
}
