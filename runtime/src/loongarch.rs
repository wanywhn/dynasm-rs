
//! Runtime support for the LoongArch architecture assembling target.

use byteorder::{ByteOrder, LittleEndian};

use crate::Register;
use crate::relocations::{fits_signed_bitfield, ImpossibleRelocation, Relocation, RelocationKind, RelocationSize};

/// Relocation implementation for the LoongArch architecture.
#[derive(Debug, Clone)]
#[allow(missing_docs)]
pub enum LoongArchRelocation {
    // Branch instructions (beq, bne, jirl)
    // 16-bit offset, 2-bit aligned
    B,
    // Branch instructions (beqz, bnez, bceqz, bcnez)
    // 21-bit offset, 2-bit aligned
    BZ,
    // Jump instructions (b, bl)
    // 26-bit offset, 2-bit aligned
    J,
    // PC-relative load/store
    // 32-bit offset
    PC32,

    // 20-bit offset, 2-bit aligned
    SI20,
    // 14-bit offset, 2-bit aligned
    SI14,
    // 16-bit offset,
    SI16,
    // 12-bit offset,
    SI12,
    Plain(RelocationSize),
}

impl LoongArchRelocation {
    fn op_mask(&self) -> u32 {
        match self {
            Self::B => 0xFC00_0000,
            Self::BZ => 0xFC00_03FF,
            Self::J => 0xFC00_03FF,
            Self::PC32 => panic!("unimplemented"),
            Self::SI20 => 0xFE00_001F,
            Self::SI14 => 0xFF00_03FF,
            Self::SI16 => 0xFC00_03FF,
            Self::SI12 => 0xFFC0_03FF,
            Self::Plain(_) => 0
        }
    }
    fn encode(&self, value: isize) -> Result<u32, ImpossibleRelocation> {
        let value = i64::try_from(value).map_err(|_| ImpossibleRelocation { } )?;
        Ok(match self {
            Self::B => {
                if value & 3 != 0 || !fits_signed_bitfield(value >> 2, 16) {
                    return Err(ImpossibleRelocation { } );
                }
                let value = (value >> 2) as u32;
                (value & 0xFFFF) << 10
            },
            Self::BZ => {
                if value & 3 != 0 || !fits_signed_bitfield(value >> 2, 21) {
                    return Err(ImpossibleRelocation { } );
                }
                let value = (value >> 2) as u32;
                (value & 0xFFFF) << 10 | ((value >> 16) & 0x1F)
            },
            Self::J => {
                if value & 3 != 0 || !fits_signed_bitfield(value >> 2, 26) {
                    return Err(ImpossibleRelocation { } );
                }
                let value = (value >> 2) as u32;
                ((value & 0xFFFF) << 10) | ((value >> 16) & 0x1F)
            },
            Self::SI20 => {
                if value & 3 != 0 || !fits_signed_bitfield(value >> 2, 20) {
                    return Err(ImpossibleRelocation { } );
                }
                let value = (value >> 2) as u32;
                (value & 0xF_FFFF) << 5

            },
            Self::SI14 => {
                if value & 3 != 0 || !fits_signed_bitfield(value >> 2, 14) {
                    return Err(ImpossibleRelocation { } );
                }
                let value = (value >> 2) as u32;
                (value & 0x3FFF) << 10
            },
            Self::SI16 => {
                if !fits_signed_bitfield(value, 16) {
                    return Err(ImpossibleRelocation { } );
                }
                (value as u32 & 0xFFFF) << 10
            },
            Self::SI12 => {
                if !fits_signed_bitfield(value, 12) {
                    return Err(ImpossibleRelocation { } );
                }
                (value as u32 & 0xFFF) << 10
            },
            Self::PC32 => panic!("unimplemented"),
            Self::Plain(_) => return Err(ImpossibleRelocation { } )
        })
    }
}

impl Relocation for LoongArchRelocation {
    type Encoding = (u8,);
    fn from_encoding(encoding: Self::Encoding) -> Self {
        match encoding.0 {
        0 => Self::B,
        1 => Self::BZ,
        2 => Self::J,
        3 => Self::PC32,
        4 => Self::SI20,
        5 => Self::SI20,
        6 => Self::SI14,
        7 => Self::SI16,
        8 => Self::SI12,
        x => Self::Plain(RelocationSize::from_encoding(x-8))
        }
    }
    fn from_size(size: RelocationSize) -> Self {
        Self::Plain(size)
    }
    fn size(&self) -> usize {
        match self {
            Self::Plain(s) => s.size(),
            _ => RelocationSize::DWord.size(),
        }
    }
    fn write_value(&self, buf: &mut [u8], value: isize) -> Result<(), ImpossibleRelocation> {
        if let Self::Plain(s) = self {
            return s.write_value(buf, value);
        };
        let mask = self.op_mask();
        let template = LittleEndian::read_u32(buf) & mask;

        let packed = self.encode(value)?;

        LittleEndian::write_u32(buf, template | packed);
        Ok(())
    }
    fn read_value(&self, buf: &[u8]) -> isize {
        if let Self::Plain(s) = self {
            return s.read_value(buf);
        };
        let mask = !self.op_mask();
        let value = LittleEndian::read_u32(buf);
        let unpacked = match self {
            Self::B => u64::from(
                (value & mask) >> 16
            ) << 2,
            Self::BZ => {
                let value = value & mask;
                let tvalue = (value & 0x1F) << 16 | (value & 0xffff);
                u64::from( tvalue ) << 2
            },
            Self::J  => {
                let value = value & mask;
                let tvalue = (value & 0x3FF) << 16 | (value & 0xffff);
                u64::from( tvalue ) << 2
            },
            Self::SI20 => u64::from(
                (value & mask) >> 5
            ) << 2,
            Self::SI14 => u64::from(
                (value & mask) >> 10
            ) << 2,
            Self::SI16 => u64::from(
                (value & mask) >> 10
            ),
            Self::SI12 => u64::from(
                (value & mask) >> 10
            ),
            Self::PC32 => panic!("unimplemented"),
            Self::Plain(_) => unreachable!()
        };

        // Sign extend.
        let bits = match self {
            Self::B => 18,
            Self::BZ => 23,
            Self::J => 28,
            Self::SI20 => 22,
            Self::SI14 => 16,
            Self::SI16 => 16,
            Self::SI12 => 12,
            Self::PC32 => unreachable!(),
            Self::Plain(_) => unreachable!()
        };
        let offset = 1u64 << (bits - 1);
        let value: u64 = (unpacked ^ offset).wrapping_sub(offset);

        value as i64 as isize
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

/// 1, 2, 4, 8 or 16-bytes scalar FP / vector SIMD registers. 
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RV {
    V0 = 0x00, V1 = 0x01, V2 = 0x02, V3 = 0x03,
    V4 = 0x04, V5 = 0x05, V6 = 0x06, V7 = 0x07,
    V8 = 0x08, V9 = 0x09, V10= 0x0A, V11= 0x0B,
    V12= 0x0C, V13= 0x0D, V14= 0x0E, V15= 0x0F,
    V16= 0x10, V17= 0x11, V18= 0x12, V19= 0x13,
    V20= 0x14, V21= 0x15, V22= 0x16, V23= 0x17,
    V24= 0x18, V25= 0x19, V26= 0x1A, V27= 0x1B,
    V28= 0x1C, V29= 0x1D, V30= 0x1E, V31= 0x1F,
}
reg_impls!(RV);
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_assembler_creation() {
        // Basic test to verify assembler can be created
        let _ = Assembler::new();
    }
}
