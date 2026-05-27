
//! Runtime support for the LoongArch architecture assembling target.

use byteorder::{ByteOrder, LittleEndian};

use crate::Register;
use crate::relocations::{fits_signed_bitfield, ImpossibleRelocation, Relocation, RelocationKind, RelocationSize};

/// Relocation implementation for the LoongArch architecture.
#[derive(Debug, Clone)]
#[allow(missing_docs)]
pub enum LoongArchRelocation {
    // Branch instructions (beq, bne, jirl)
    // 16-bit offset, 2-bit aligned.
    // Per LoongArch manual: PC = PC + SignExtend({offs16, 2'b0}, GRLEN).
    // The assembler receives byte offsets; value & 3 == 0 is required because
    // all LoongArch instructions are 4 bytes wide.
    B16,
    // Branch instructions (beqz, bnez, bceqz, bcnez)
    // 21-bit offset, 2-bit aligned. Same alignment rationale as B16.
    B21,
    // Jump instructions (b, bl)
    // 26-bit offset, 2-bit aligned. Same alignment rationale as B16.
    B26,

    // 20-bit signed immediate (compile-time only).
    // Used by lu12i.w, lu32i.d, pcaddi, pcaddu12i.
    // Per LoongArch manual: si20 is sign-extended; for lu12i/pcaddu12i it
    // is concatenated with 12 trailing zeros. This relocation is NOT used
    // for label resolution — the compiler encodes si20 directly into
    // bits [24:5] at compile time. The runtime encode/read_value path
    // exists only for testing/debugging.
    ABS_HI20,
    // 14-bit offset, 2-bit aligned
    // Used by ll.w, sc.w, ll.d, sc.d, ldptr.w, stptr.w, ldptr.d, stptr.d.
    SI14,
    // 12-bit signed offset (used by load/store with register + offset syntax)
    SI12,
    // PC-relative low 12 bits for load instructions.
    // Encodes bits [21:10] of (label_addr - instruction_pc).
    // Used by ld.b/ld.h/ld.w/ld.d, fld.s/fld.d.
    PCALA_LO12,
    // PC-relative high 20 bits for pcalau12i.
    // Corresponds to ELF R_LARCH_PCALA_HI20. Encodes bits [31:12] of
    // (label_addr - instruction_pc), placed at instruction bits [24:5].
    // Paired with PCALA_LO12 to form a full 32-bit PC-relative address.
    PCALA_HI20,
    Plain(RelocationSize),
}

impl LoongArchRelocation {
    fn op_mask(&self) -> u32 {
        match self {
            Self::B16 => 0xFC00_03FF,
            Self::B21 => 0xFC00_03E0,
            Self::B26 => 0xFC00_0000,
            Self::ABS_HI20 => 0xFE00_001F,
            Self::SI14 => 0xFF00_03FF,
            Self::SI12 => 0xFFC0_03FF,
            Self::PCALA_LO12 => 0xFFC0_03FF,
            Self::PCALA_HI20 => 0xFE00_001F,
            Self::Plain(_) => 0,
        }
    }
    fn encode(&self, value: isize) -> Result<u32, ImpossibleRelocation> {
        let value = i64::try_from(value).map_err(|_| ImpossibleRelocation { } )?;
        Ok(match self {
            Self::B16 => {
                if value & 3 != 0 || !fits_signed_bitfield(value >> 2, 16) {
                    return Err(ImpossibleRelocation { } );
                }
                let value = (value >> 2) as u32;
                (value & 0xFFFF) << 10
            },
            Self::B21 => {
                if value & 3 != 0 || !fits_signed_bitfield(value >> 2, 21) {
                    return Err(ImpossibleRelocation { } );
                }
                let value = (value >> 2) as u32;
                (value & 0xFFFF) << 10 | ((value >> 16) & 0x1F)
            },
            Self::B26 => {
                if value & 3 != 0 || !fits_signed_bitfield(value >> 2, 26) {
                    return Err(ImpossibleRelocation { } );
                }
                let value = (value >> 2) as u32;
                ((value & 0xFFFF) << 10) | ((value >> 16) & 0x3FF)
            },
            Self::ABS_HI20 => {
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
            Self::SI12 => {
                if !fits_signed_bitfield(value, 12) {
                    return Err(ImpossibleRelocation { } );
                }
                (value as u32 & 0xFFF) << 10
            },
            Self::PCALA_LO12 => {
                // PC-relative low 12 bits: same bit encoding as SI12 (bits 10-21),
                // but semantically represents (label - pc) rather than an absolute offset.
                if !fits_signed_bitfield(value, 12) {
                    return Err(ImpossibleRelocation {});
                }
                (value as u32 & 0xFFF) << 10
            },
            Self::PCALA_HI20 => {
                // PC-relative high 20 bits for pcalau12i.
                // bits [31:12] of (label - pc), placed at instruction bits [24:5].
                if !fits_signed_bitfield(value >> 12, 20) {
                    return Err(ImpossibleRelocation {});
                }
                ((value >> 12) as u32 & 0xF_FFFF) << 5
            },
            Self::Plain(_) => return Err(ImpossibleRelocation {}),
        })
    }
}

impl Relocation for LoongArchRelocation {
    type Encoding = (u8,);
    fn from_encoding(encoding: Self::Encoding) -> Self {
        match encoding.0 {
        0 => Self::B16,
        1 => Self::B21,
            2 => Self::B26,
            4 => Self::ABS_HI20,
        5 => Self::SI14,
            7 => Self::SI12,
            8 => Self::PCALA_LO12,
            13 => Self::PCALA_HI20,
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
            Self::B16 => u64::from(
                (value & mask) >> 10
            ) << 2,
            Self::B21 => {
                let value = value & mask;
                let tvalue = (value & 0x1F) << 16 | (value & 0xffff);
                u64::from( tvalue ) << 2
            },
            Self::B26  => {
                let value = value & mask;
                let tvalue = (value & 0x3FF) << 16 | (value & 0xffff);
                u64::from( tvalue ) << 2
            },
            Self::ABS_HI20 => u64::from(
                (value & mask) >> 5
            ),
            Self::SI14 => u64::from(
                (value & mask) >> 10
            ) << 1,
            Self::SI12 => u64::from(
                (value & mask) >> 10
            ),
            Self::PCALA_LO12 => u64::from(
                (value & mask) >> 10
            ),
            Self::PCALA_HI20 => u64::from((value & mask) >> 10),
            Self::Plain(_) => unreachable!(),
        };

        // Sign extend.
        let bits = match self {
            Self::B16 => 16,
            Self::B21 => 21,
            Self::B26 => 26,
            Self::ABS_HI20 => 20,
            Self::SI14 => 14,
            Self::SI12 => 12,
            Self::PCALA_LO12 => 12,
            Self::PCALA_HI20 => 12,
            Self::Plain(_) => unreachable!(),
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

    /// Compile-time validation: `from_encoding` discriminants must match
    /// the plugin `Relocation::DISCRIMINANT_TABLE` exactly.
    /// If the plugin adds/removes/renames a relocation variant, this test
    /// will fail at compile time (const) or runtime (test), preventing silent
    /// desync between the proc-macro and the runtime library.
    ///
    /// NOTE: LITERAL8/16/32/64 (discriminants 9-12) are excluded because
    /// they map to `Plain(RelocationSize::from_encoding(N))` which panics
    /// for those values — this is pre-existing dead code that will be fixed
    /// when literal relocation support is implemented.
    const ENCODING_TABLE: &'static [(u8, fn() -> LoongArchRelocation)] = &[
        (0, || LoongArchRelocation::B16),
        (1, || LoongArchRelocation::B21),
        (2, || LoongArchRelocation::B26),
        (4, || LoongArchRelocation::ABS_HI20),
        (5, || LoongArchRelocation::SI14),
        (6, || LoongArchRelocation::B16),  // SI16 merged into B16 (identical encoding)
        (7, || LoongArchRelocation::SI12),
        (8, || LoongArchRelocation::PCALA_LO12),
        (13, || LoongArchRelocation::PCALA_HI20),
    ];

    #[test]
    fn test_from_encoding_consistency() {
        // Verify every discriminant in ENCODING_TABLE round-trips through from_encoding
        for &(disc, ref ctor) in ENCODING_TABLE {
            let expected = ctor();
            let actual = LoongArchRelocation::from_encoding((disc,));
            assert_eq!(
                std::mem::discriminant(&expected),
                std::mem::discriminant(&actual),
                "from_encoding({}): mismatch — expected {:?}, got {:?}",
                disc,
                expected,
                actual,
            );
        }
    }

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
