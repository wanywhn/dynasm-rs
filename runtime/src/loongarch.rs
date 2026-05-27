
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

    // 20-bit offset
    SI20,
    // 14-bit offset, 2-bit aligned
    SI14,
    // 16-bit offset, 2-bit aligned
    SI16,
    // 12-bit offset,
    SI12,
    // PC-relative low 12 bits for load instructions (signed)
    PCLO12,
    // PC-relative low 12 bits for store instructions (signed)
    PCLO12S,
    Plain(RelocationSize),
}

impl LoongArchRelocation {
    fn op_mask(&self) -> u32 {
        match self {
            Self::B => 0xFC00_03FF,
            Self::BZ => 0xFC00_03E0,
            Self::J => 0xFC00_0000,
            // PC32 is a plain 32-bit value — mask=0 means the entire
            // instruction word is overwritten by encode().
            Self::PC32 => 0,
            Self::SI20 => 0xFE00_001F,
            Self::SI14 => 0xFF00_03FF,
            Self::SI16 => 0xFC00_03FF,
            Self::SI12 => 0xFFC0_03FF,
            Self::PCLO12 => 0xFFC0_03FF,
            Self::PCLO12S => 0xFFC0_03FF,
            Self::Plain(_) => 0,
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
                ((value & 0xFFFF) << 10) | ((value >> 16) & 0x3FF)
            },
            Self::SI20 => {
                if !fits_signed_bitfield(value, 20) {
                    return Err(ImpossibleRelocation { } );
                }
                ((value >> 2) as u32  & 0xF_FFFF) << 5
            },
            Self::SI14 => {
                if value & 3 != 0 || !fits_signed_bitfield(value >> 2, 14) {
                    return Err(ImpossibleRelocation { } );
                }
                let value = (value >> 2) as u32;
                (value & 0x3FFF) << 10
            },
            Self::SI16 => {
                if value & 3 != 0 || !fits_signed_bitfield(value >> 2, 16) {
                    return Err(ImpossibleRelocation { } );
                }
                let value = (value >> 2) as u32;
                (value as u32 & 0xFFFF) << 10
            },
            Self::SI12 => {
                if !fits_signed_bitfield(value, 12) {
                    return Err(ImpossibleRelocation { } );
                }
                (value as u32 & 0xFFF) << 10
            },
            Self::PCLO12 | Self::PCLO12S => {
                // PC-relative low 12 bits: same bit encoding as SI12 (bits 10-21),
                // but semantically represents (label - pc) rather than an absolute offset.
                if !fits_signed_bitfield(value, 12) {
                    return Err(ImpossibleRelocation { } );
                }
                (value as u32 & 0xFFF) << 10
            },
            // PC32 is a raw 32-bit signed offset (no alignment requirement).
            // Used for PC-relative load/store placeholder values.
            Self::PC32 => {
                if !fits_signed_bitfield(value, 32) {
                    return Err(ImpossibleRelocation { } );
                }
                value as u32
            },
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
        5 => Self::SI14,
        6 => Self::SI16,
7 => Self::SI12,
            8 => Self::PCLO12,
            13 => Self::PCLO12S,
            9 => Self::Plain(RelocationSize::from_encoding(9)),
            10 => Self::Plain(RelocationSize::from_encoding(10)),
            11 => Self::Plain(RelocationSize::from_encoding(11)),
            12 => Self::Plain(RelocationSize::from_encoding(12)),
            x => Self::Plain(RelocationSize::from_encoding(x)),
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
                (value & mask) >> 10
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
            ),
            Self::SI14 => u64::from(
                (value & mask) >> 10
            ) << 1,
            Self::SI16 => u64::from(
                (value & mask) >> 10
            ),
            Self::SI12 => u64::from(
                (value & mask) >> 10
            ),
            Self::PCLO12 => u64::from(
                (value & mask) >> 10
            ),
            Self::PCLO12S => u64::from(
                (value & mask) >> 10
            ),
            // PC32 is a raw 32-bit value — just read it directly.
            Self::PC32 => u64::from(value),
            Self::Plain(_) => unreachable!()
        };

        // Sign extend.
        let bits = match self {
            Self::B => 16,
            Self::BZ => 21,
            Self::J => 26,
            Self::SI20 => 20,
            Self::SI14 => 14,
            Self::SI16 => 16,
            Self::SI12 => 12,
            Self::PCLO12 => 12,
            Self::PCLO12S => 12,
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

/// Floating point registers (F0-F31)
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FR {
    F0 = 0x00, F1 = 0x01, F2 = 0x02, F3 = 0x03,
    F4 = 0x04, F5 = 0x05, F6 = 0x06, F7 = 0x07,
    F8 = 0x08, F9 = 0x09, F10= 0x0A, F11= 0x0B,
    F12= 0x0C, F13= 0x0D, F14= 0x0E, F15= 0x0F,
    F16= 0x10, F17= 0x11, F18= 0x12, F19= 0x13,
    F20= 0x14, F21= 0x15, F22= 0x16, F23= 0x17,
    F24= 0x18, F25= 0x19, F26= 0x1A, F27= 0x1B,
    F28= 0x1C, F29= 0x1D, F30= 0x1E, F31= 0x1F,
}
reg_impls!(FR);

/// Floating point condition code registers (FCC0-FCC7)
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FCCR {
    FCC0 = 0x00, FCC1 = 0x01, FCC2 = 0x02, FCC3 = 0x03,
    FCC4 = 0x04, FCC5 = 0x05, FCC6 = 0x06, FCC7 = 0x07,
}
reg_impls!(FCCR);

/// Floating point control/status registers (FCSR0-FCSR3)
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FCSR {
    FCSR0 = 0x00, FCSR1 = 0x01, FCSR2 = 0x02, FCSR3 = 0x03,
}
reg_impls!(FCSR);

/// LASX 256-bit vector registers (X0-X31)
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum XR {
    X0 = 0x00, X1 = 0x01, X2 = 0x02, X3 = 0x03,
    X4 = 0x04, X5 = 0x05, X6 = 0x06, X7 = 0x07,
    X8 = 0x08, X9 = 0x09, X10= 0x0A, X11= 0x0B,
    X12= 0x0C, X13= 0x0D, X14= 0x0E, X15= 0x0F,
    X16= 0x10, X17= 0x11, X18= 0x12, X19= 0x13,
    X20= 0x14, X21= 0x15, X22= 0x16, X23= 0x17,
    X24= 0x18, X25= 0x19, X26= 0x1A, X27= 0x1B,
    X28= 0x1C, X29= 0x1D, X30= 0x1E, X31= 0x1F,
}
reg_impls!(XR);

/// Handler for `u32` out-of-range LoongArch immediates.
#[inline(never)]
pub fn immediate_out_of_range_unsigned_32(immediate: u32) -> ! {
    panic!("Cannot assemble this LoongArch instruction. Immediate {immediate} is out of range.")
}

/// Handler for `i32` out-of-range LoongArch immediates.
#[inline(never)]
pub fn immediate_out_of_range_signed_32(immediate: i32) -> ! {
    panic!("Cannot assemble this LoongArch instruction. Immediate {immediate} is out of range.")
}

/// Handler for `u64` out-of-range LoongArch immediates.
#[inline(never)]
pub fn immediate_out_of_range_unsigned_64(immediate: u64) -> ! {
    panic!("Cannot assemble this LoongArch instruction. Immediate {immediate} is out of range.")
}

/// Handler for `i64` out-of-range LoongArch immediates.
#[inline(never)]
pub fn immediate_out_of_range_signed_64(immediate: i64) -> ! {
    panic!("Cannot assemble this LoongArch instruction. Immediate {immediate} is out of range.")
}

/// Handler for invalid register number (e.g. r0 used where r0 is forbidden).
#[inline(never)]
pub fn invalid_register(register: u8) -> ! {
    panic!("Cannot assemble this LoongArch instruction. Register number {register} is invalid for this operand.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_assembler_creation() {
        let _ = Assembler::new();
    }

    #[test]
    #[should_panic(expected = "LoongArch")]
    fn test_immediate_unsigned_32_out_of_range() {
        immediate_out_of_range_unsigned_32(999);
    }

    #[test]
    #[should_panic(expected = "LoongArch")]
    fn test_immediate_signed_32_out_of_range() {
        immediate_out_of_range_signed_32(-999);
    }

    #[test]
    #[should_panic(expected = "LoongArch")]
    fn test_immediate_unsigned_64_out_of_range() {
        immediate_out_of_range_unsigned_64(999);
    }

    #[test]
    #[should_panic(expected = "LoongArch")]
    fn test_immediate_signed_64_out_of_range() {
        immediate_out_of_range_signed_64(-999);
    }

    #[test]
    #[should_panic(expected = "LoongArch")]
    fn test_invalid_register() {
        invalid_register(0);
    }

    #[test]
    fn test_error_messages_contain_loongarch() {
        // Verify error functions reference LoongArch, not other architectures
        // These must panic with "LoongArch" in the message
        let msg_u32 = std::panic::catch_unwind(|| immediate_out_of_range_unsigned_32(0));
        assert!(msg_u32.is_err());
        if let Err(p) = msg_u32 {
            if let Some(s) = p.downcast_ref::<String>() {
                assert!(s.contains("LoongArch"), "Error message should reference LoongArch, got: {s}");
                assert!(!s.contains("Aarch64"), "Error message should NOT reference Aarch64, got: {s}");
                assert!(!s.contains("RISC-V"), "Error message should NOT reference RISC-V, got: {s}");
            }
        }

        let msg_i32 = std::panic::catch_unwind(|| immediate_out_of_range_signed_32(0));
        assert!(msg_i32.is_err());
        if let Err(p) = msg_i32 {
            if let Some(s) = p.downcast_ref::<String>() {
                assert!(s.contains("LoongArch"), "Error message should reference LoongArch, got: {s}");
            }
        }

        let msg_reg = std::panic::catch_unwind(|| invalid_register(0));
        assert!(msg_reg.is_err());
        if let Err(p) = msg_reg {
            if let Some(s) = p.downcast_ref::<String>() {
                assert!(s.contains("LoongArch"), "Error message should reference LoongArch, got: {s}");
            }
        }
    }
}
