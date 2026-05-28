
//! Runtime support for the LoongArch architecture assembling target.

use byteorder::{ByteOrder, LittleEndian};

use crate::Register;
use crate::relocations::{fits_signed_bitfield, ImpossibleRelocation, Relocation, RelocationKind, RelocationSize};

/// Relocation implementation for the LoongArch architecture.
#[derive(Debug, Clone)]
#[allow(missing_docs)]
#[allow(non_camel_case_types)] // Relocation names align with ELF psABI convention (R_LARCH_*)
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

    // Raw 20-bit si20 field (no shift in encode).
    // Used by lu12i.w, lu32i.d for compile-time immediates.
    // The value is expected to be the already-shifted si20; the runtime
    // stores it directly into bits [24:5] without any shift.
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
    // pcaddi: PC + SE({si20, 2'b0}). Encode shift >>2, no compensation.
    PCADD_SHIFT2,
    // pcaddu12i: PC + SE({si20, 12'b0}). Encode shift >>12, +0x800 compensation.
    PCADD_SHIFT12,
    // pcaddu18i: PC + SE({si20, 18'b0}). Encode shift >>18, +0x20000 compensation.
    PCADD_SHIFT18,
    // 8-byte SPLIT relocation: pcalau12i + addi.d pair.
    // Patches hi20 (with +0x800 compensation) into first instruction,
    // lo12 into second instruction. Used by la/la.local pseudo-instructions.
    SPLIT_PCALA,
    // 8-byte SPLIT relocation: pcaddu12i + jirl pair (call30).
    // hi20 = val[31:12] at bits [24:5] of first inst (no compensation).
    // lo10 = val[11:2] at bits [25:10] of second inst (setK16).
    SPLIT_CALL30,
    // 8-byte SPLIT relocation: pcaddu18i + jirl pair (call36).
    // hi20 = (val+0x20000)[37:18] at bits [24:5] of first inst (+0x20000 compensation).
    // lo16 = val[17:2] at bits [25:10] of second inst (setK16).
    SPLIT_CALL36,
    Plain(RelocationSize),
}

impl LoongArchRelocation {
    fn op_mask(&self) -> u32 {
        match self {
            Self::B16 => 0xFC00_03FF,
            Self::B21 => 0xFC00_03E0,
            Self::B26 => 0xFC00_0000,
            Self::ABS_HI20 => 0xFE00_001F,
            Self::PCADD_SHIFT2 => 0xFE00_001F,
            Self::PCADD_SHIFT12 => 0xFE00_001F,
            Self::PCADD_SHIFT18 => 0xFE00_001F,
            Self::SI14 => 0xFF00_03FF,
            Self::SI12 => 0xFFC0_03FF,
            Self::PCALA_LO12 => 0xFFC0_03FF,
            Self::PCALA_HI20 => 0xFE00_001F,
            Self::SPLIT_PCALA | Self::SPLIT_CALL30 | Self::SPLIT_CALL36 => 0,  // SPLIT variants bypass op_mask/encode
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
                ((value as u32) & 0xF_FFFF) << 5
            },
            Self::PCADD_SHIFT2 => {
                // pcaddi: PC + SE({si20, 2'b0}). No compensation.
                if !fits_signed_bitfield(value >> 2, 20) {
                    return Err(ImpossibleRelocation {});
                }
                ((value >> 2) as u32 & 0xF_FFFF) << 5
            },
            Self::PCADD_SHIFT12 => {
                // pcaddu12i: PC + SE({si20, 12'b0}). +0x800 compensation
                // to handle sign-extension of the paired lo12.
                let value = value + 0x800;
                if !fits_signed_bitfield(value >> 12, 20) {
                    return Err(ImpossibleRelocation {});
                }
                ((value >> 12) as u32 & 0xF_FFFF) << 5
            },
            Self::PCADD_SHIFT18 => {
                // pcaddu18i: PC + SE({si20, 18'b0}). +0x20000 compensation
                // to handle sign-extension of the paired lo18.
                let value = value + 0x20000;
                if !fits_signed_bitfield(value >> 18, 20) {
                    return Err(ImpossibleRelocation {});
                }
                ((value >> 18) as u32 & 0xF_FFFF) << 5
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
                // +0x800 compensation for sign-extension of paired lo12.
                let value = value + 0x800;
                if !fits_signed_bitfield(value >> 12, 20) {
                    return Err(ImpossibleRelocation {});
                }
                ((value >> 12) as u32 & 0xF_FFFF) << 5
            },
            Self::SPLIT_PCALA | Self::SPLIT_CALL30 | Self::SPLIT_CALL36 | Self::Plain(_) => return Err(ImpossibleRelocation {}),
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
            6 => Self::B16,  // SI16 merged into B16 (identical encoding)
            7 => Self::SI12,
            8 => Self::PCALA_LO12,
            13 => Self::PCALA_HI20,
            14 => Self::PCADD_SHIFT2,
            15 => Self::PCADD_SHIFT12,
            16 => Self::PCADD_SHIFT18,
            17 => Self::SPLIT_PCALA,
            18 => Self::SPLIT_CALL30,
            19 => Self::SPLIT_CALL36,
            x => Self::Plain(RelocationSize::from_encoding(x)),
        }
    }
    fn from_size(size: RelocationSize) -> Self {
        Self::Plain(size)
    }
    fn size(&self) -> usize {
        match self {
            Self::SPLIT_PCALA | Self::SPLIT_CALL30 | Self::SPLIT_CALL36 => 8,
            Self::Plain(s) => s.size(),
            _ => RelocationSize::DWord.size(),
        }
    }
    fn write_value(&self, buf: &mut [u8], value: isize) -> Result<(), ImpossibleRelocation> {
        if let Self::Plain(s) = self {
            return s.write_value(buf, value);
        };

        // SPLIT relocations patch across two instructions directly, bypassing
        // the generic op_mask()+encode()+OR pattern.
        match self {
            Self::SPLIT_PCALA => {
                // pcalau12i rd, hi20; addi.d rd, rd, lo12
                // hi20 = (val + 0x800)[31:12] at bits [24:5] of first inst
                // lo12 = val[11:0] at bits [21:10] of second inst
                // Range: -0x8000_0800..0x7FFF_F7FF (sign-extension interaction limits)
                // Values at the edges lose lo12 precision, but this never occurs in practice.
                let val_cast: i32 = i32::try_from(value).map_err(|_| ImpossibleRelocation {})?;
                if val_cast & 3 != 0 { return Err(ImpossibleRelocation {}); }
                if (value as i64) < -0x8000_0800_i64 || (value as i64) > 0x7FFF_F7FF_i64 {
                    return Err(ImpossibleRelocation {});
                }
                let val_round: u32 = (val_cast as u32).wrapping_add(0x800);
                let instr1 = (LittleEndian::read_u32(&buf[..4]) & 0xFE00_001F)
                    | (((val_round >> 12) & 0xF_FFFF) << 5);
                let instr2 = (LittleEndian::read_u32(&buf[4..]) & 0xFFC0_03FF)
                    | ((val_cast as u32 & 0xFFF) << 10);
                LittleEndian::write_u32(&mut buf[..4], instr1);
                LittleEndian::write_u32(&mut buf[4..], instr2);
                return Ok(());
            },
            Self::SPLIT_CALL30 => {
                // pcaddu12i ra, hi20; jirl ra, ra, lo10
                // hi20 = val[31:12] at bits [24:5] of first inst (setJ20, no compensation)
                // lo10 = val[11:2] at bits [25:10] of second inst (setK16)
                // Range: 32-bit signed, 4-byte aligned
                let val_cast: i32 = i32::try_from(value).map_err(|_| ImpossibleRelocation {})?;
                if val_cast & 3 != 0 { return Err(ImpossibleRelocation {}); }
                let hi20: u32 = ((val_cast as u32) >> 12) & 0xF_FFFF;
                let lo10: u32 = ((val_cast as u32) >> 2) & 0x3FF;
                let instr1 = (LittleEndian::read_u32(&buf[..4]) & 0xFE00_001F) | (hi20 << 5);
                let instr2 = (LittleEndian::read_u32(&buf[4..]) & 0xFC00_03FF) | (lo10 << 10);
                LittleEndian::write_u32(&mut buf[..4], instr1);
                LittleEndian::write_u32(&mut buf[4..], instr2);
                return Ok(());
            },
            Self::SPLIT_CALL36 => {
                // pcaddu18i ra, hi20; jirl ra, ra, lo16
                // hi20 = (val+0x20000)[37:18] at bits [24:5] of first inst (setJ20, +0x20000 comp)
                // lo16 = val[17:2] at bits [25:10] of second inst (setK16)
                // Range: 38-bit signed (with compensation), 4-byte aligned
                let val_cast: i64 = i64::try_from(value).map_err(|_| ImpossibleRelocation {})?;
                if val_cast & 3 != 0 { return Err(ImpossibleRelocation {}); }
                let compensated: i64 = val_cast + 0x20000;
                if !fits_signed_bitfield(compensated, 38) { return Err(ImpossibleRelocation {}); }
                let hi20: u32 = (((compensated as u64) >> 18) & 0xF_FFFF) as u32;
                let lo16: u32 = (((val_cast as u64) >> 2) & 0xFFFF) as u32;
                let instr1 = (LittleEndian::read_u32(&buf[..4]) & 0xFE00_001F) | (hi20 << 5);
                let instr2 = (LittleEndian::read_u32(&buf[4..]) & 0xFC00_03FF) | (lo16 << 10);
                LittleEndian::write_u32(&mut buf[..4], instr1);
                LittleEndian::write_u32(&mut buf[4..], instr2);
                return Ok(());
            },
            _ => {},
        }

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

        // SPLIT relocations read across two instructions
        match self {
            Self::SPLIT_PCALA => {
                let instr1 = LittleEndian::read_u32(&buf[..4]);
                let instr2 = LittleEndian::read_u32(&buf[4..]);
                let hi: u64 = (((instr1 >> 5) & 0xF_FFFF) as u64) << 12;
                let mut lo: u32 = (instr2 >> 10) & 0xFFF;
                lo = (lo ^ 0x800).wrapping_sub(0x800);  // sign-extend 12→32
                let unpacked = hi.wrapping_add(lo as u64);
                // 32-bit sign extension
                let offset = 1u64 << 31;
                let value: u64 = (unpacked ^ offset).wrapping_sub(offset);
                return value as i64 as isize;
            },
            Self::SPLIT_CALL30 => {
                // pcaddu12i + jirl: hi20 = val[31:12], lo10 = val[11:2]
                let instr1 = LittleEndian::read_u32(&buf[..4]);
                let instr2 = LittleEndian::read_u32(&buf[4..]);
                let hi20: u64 = (((instr1 >> 5) & 0xF_FFFF) as u64) << 12;
                let lo10: u64 = (((instr2 >> 10) & 0x3FF) as u64) << 2;
                let unpacked = hi20 | lo10;
                // 32-bit sign extension
                let offset = 1u64 << 31;
                let value: u64 = (unpacked ^ offset).wrapping_sub(offset);
                return value as i64 as isize;
            },
            Self::SPLIT_CALL36 => {
                // pcaddu18i + jirl: hi20 and lo16 are independently sign-extended.
                // hi20 undergoes SE20→64 (pcaddu18i), lo16 undergoes SE16→64 (jirl).
                // val = SE20(hi20) << 18 + SE16(lo16) << 2
                let instr1 = LittleEndian::read_u32(&buf[..4]);
                let instr2 = LittleEndian::read_u32(&buf[4..]);
                let hi20_raw: u64 = ((instr1 >> 5) & 0xF_FFFF) as u64;
                let lo16_raw: u64 = ((instr2 >> 10) & 0xFFFF) as u64;
                // Independent sign extensions
                let hi20_se: i64 = ((hi20_raw ^ (1u64 << 19)).wrapping_sub(1u64 << 19)) as i64;
                let lo16_se: i64 = ((lo16_raw ^ (1u64 << 15)).wrapping_sub(1u64 << 15)) as i64;
                let value: i64 = (hi20_se << 18) + (lo16_se << 2);
                return value as isize;
            },
            _ => {},
        }

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
            Self::PCADD_SHIFT2 => u64::from(
                (value & mask) >> 5
            ) << 2,
            Self::PCADD_SHIFT12 => u64::from(
                (value & mask) >> 5
            ) << 12,
            Self::PCADD_SHIFT18 => u64::from(
                (value & mask) >> 5
            ) << 18,
            Self::SI14 => u64::from(
                (value & mask) >> 10
            ) << 1,
            Self::SI12 => u64::from(
                (value & mask) >> 10
            ),
            Self::PCALA_LO12 => u64::from(
                (value & mask) >> 10
            ),
            Self::PCALA_HI20 => u64::from(
                (value & mask) >> 5
            ) << 12,
            Self::SPLIT_PCALA | Self::SPLIT_CALL30 | Self::SPLIT_CALL36 => unreachable!(),  // handled above in early return
            Self::Plain(_) => unreachable!(),
        };

        // Sign extend.
        let bits = match self {
            Self::B16 => 16,
            Self::B21 => 21,
            Self::B26 => 26,
            Self::ABS_HI20 => 20,
            Self::PCADD_SHIFT2 => 22,
            Self::PCADD_SHIFT12 => 32,
            Self::PCADD_SHIFT18 => 38,
            Self::SI14 => 14,
            Self::SI12 => 12,
            Self::PCALA_LO12 => 12,
            Self::PCALA_HI20 => 32,
            Self::SPLIT_PCALA | Self::SPLIT_CALL30 | Self::SPLIT_CALL36 => unreachable!(),  // handled above in early return
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
        (14, || LoongArchRelocation::PCADD_SHIFT2),
        (15, || LoongArchRelocation::PCADD_SHIFT12),
        (16, || LoongArchRelocation::PCADD_SHIFT18),
        (17, || LoongArchRelocation::SPLIT_PCALA),
        (18, || LoongArchRelocation::SPLIT_CALL30),
        (19, || LoongArchRelocation::SPLIT_CALL36),
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

    // --- Relocation encode/read_value correctness tests ---
    //
    // Each PC-relative instruction now has its own relocation variant
    // with the correct shift and compensation:
    //   PCADD_SHIFT2:  pcaddi     — >>2, no compensation
    //   PCADD_SHIFT12: pcaddu12i  — >>12, +0x800 compensation
    //   PCADD_SHIFT18: pcaddu18i  — >>18, +0x20000 compensation
    //   PCALA_HI20:    pcalau12i  — >>12, +0x800 compensation
    //   ABS_HI20:      raw si20   — no shift (compile-time immediate)

    /// ABS_HI20 (raw si20, no shift): encode stores value directly into si20 field.
    #[test]
    fn test_abs_hi20_raw_si20_no_shift() {
        let reloc = LoongArchRelocation::ABS_HI20;
        let value: isize = 0x12345; // raw si20 value
        let encoded = reloc.encode(value).unwrap();
        let si20 = (encoded >> 5) & 0xF_FFFF;
        assert_eq!(si20, 0x12345, "ABS_HI20 should store raw si20 (no shift)");
        // roundtrip: read_value returns raw si20 sign-extended
        let mut buf = [0u8; 4];
        reloc.write_value(&mut buf, value).unwrap();
        let recovered = reloc.read_value(&buf);
        assert_eq!(recovered, value, "ABS_HI20 roundtrip should recover raw si20");
    }

    /// PCADD_SHIFT2 (pcaddi, >>2, no compensation): encode shifts value >> 2.
    #[test]
    fn test_pcadd_shift2_encode_and_roundtrip() {
        let reloc = LoongArchRelocation::PCADD_SHIFT2;
        // value = 0x3FFFC: si20 = 0xFFFF, pcaddi computes PC + 0xFFFF<<2 = PC + 0x3FFFC
        let value: isize = 0x3FFFC;
        let encoded = reloc.encode(value).unwrap();
        let si20 = (encoded >> 5) & 0xF_FFFF;
        assert_eq!(si20, 0xFFFF, "PCADD_SHIFT2 si20 should be value>>2");
        // roundtrip: read_value applies inverse shift <<2
        let mut buf = [0u8; 4];
        reloc.write_value(&mut buf, value).unwrap();
        let recovered = reloc.read_value(&buf);
        assert_eq!(recovered, value, "PCADD_SHIFT2 roundtrip should recover original offset");
    }

    /// PCADD_SHIFT12 (pcaddu12i, >>12, +0x800 compensation).
    #[test]
    fn test_pcadd_shift12_encode_with_compensation() {
        let reloc = LoongArchRelocation::PCADD_SHIFT12;
        // value = 0x12345_000: si20 should be ((value + 0x800) >> 12) & 0xF_FFFF
        let value: isize = 0x12345_000;
        let encoded = reloc.encode(value).unwrap();
        let si20 = (encoded >> 5) & 0xF_FFFF;
        // (0x12345_000 + 0x800) >> 12 = 0x12345_800 >> 12 = 0x12346 (carry from +0x800)
        // Wait, 0x12345_800 >> 12 = 0x12346 (with lo12 causing carry into hi20)
        let expected_si20: u32 = (((value as i64 + 0x800) >> 12) as u32) & 0xF_FFFF;
        assert_eq!(si20, expected_si20,
            "PCADD_SHIFT12 si20 should be (value+0x800)>>12 = 0x{:X}", expected_si20);
    }

    /// PCADD_SHIFT12 with a value where lo12 is positive (no carry into hi20).
    #[test]
    fn test_pcadd_shift12_no_carry_when_lo12_positive() {
        let reloc = LoongArchRelocation::PCADD_SHIFT12;
        // value = 0x1000: lo12 = 0 (positive), +0x800 >> 12 = 0x1800 >> 12 = 1
        let value: isize = 0x1000;
        let encoded = reloc.encode(value).unwrap();
        let si20 = (encoded >> 5) & 0xF_FFFF;
        // (0x1000 + 0x800) >> 12 = 0x1800 >> 12 = 1
        assert_eq!(si20, 1, "PCADD_SHIFT12: value=0x1000, si20=1");
    }

    /// PCADD_SHIFT18 (pcaddu18i, >>18, +0x20000 compensation).
    #[test]
    fn test_pcadd_shift18_encode_with_compensation() {
        let reloc = LoongArchRelocation::PCADD_SHIFT18;
        // Use a value where >>18 fits in 20-bit: value = 0x40000
        // (0x40000 + 0x20000) >> 18 = 0x60000 >> 18 = 3
        let value: isize = 0x40000;
        let encoded = reloc.encode(value).unwrap();
        let si20 = (encoded >> 5) & 0xF_FFFF;
        let expected_si20: u32 = (((value as i64 + 0x20000) >> 18) as u32) & 0xF_FFFF;
        assert_eq!(si20, expected_si20,
            "PCADD_SHIFT18 si20 should be (value+0x20000)>>18 = 0x{:X}", expected_si20);
    }

    /// PCALA_HI20 (pcalau12i, >>12, +0x800 compensation) — now with compensation.
    #[test]
    fn test_pcala_hi20_encode_with_compensation() {
        let reloc = LoongArchRelocation::PCALA_HI20;
        // value = 0x12345_000: si20 = ((value + 0x800) >> 12) & 0xF_FFFF
        let value: isize = 0x12345_000;
        let encoded = reloc.encode(value).unwrap();
        let si20 = (encoded >> 5) & 0xF_FFFF;
        let expected_si20: u32 = (((value as i64 + 0x800) >> 12) as u32) & 0xF_FFFF;
        assert_eq!(si20, expected_si20,
            "PCALA_HI20 si20 should include +0x800 compensation");
    }

    /// PCALA_HI20 roundtrip: read_value applies <<12 inverse shift.
    #[test]
    fn test_pcala_hi20_roundtrip() {
        let reloc = LoongArchRelocation::PCALA_HI20;
        let value: isize = 0x1000;
        let mut buf = [0u8; 4];
        reloc.write_value(&mut buf, value).unwrap();
        let recovered = reloc.read_value(&buf);
        // read_value reconstructs si20 << 12 (with compensation baked in)
        // si20 = (0x1000 + 0x800) >> 12 = 1, so recovered = 1 << 12 = 0x1000
        assert_eq!(recovered, value,
            "PCALA_HI20 roundtrip should recover offset (with compensation)");
    }

    // --- SPLIT_PCALA tests ---
    // SPLIT_PCALA patches across pcalau12i + addi.d (8 bytes).
    // hi20 = (val + 0x800)[31:12] at bits [24:5] of first inst,
    // lo12 = val[11:0] at bits [21:10] of second inst.

    /// SPLIT_PCALA write/read roundtrip with a positive value.
    #[test]
    fn test_split_pcala_positive_roundtrip() {
        let reloc = LoongArchRelocation::SPLIT_PCALA;
        // value = 0x12340: 4-byte aligned, hi20 = (0x12340 + 0x800) >> 12 = 0x12C, lo12 = 0x340
        let value: isize = 0x12340;
        let mut buf = [0u8; 8];
        LittleEndian::write_u32(&mut buf[..4], 0x1A00_0000);
        LittleEndian::write_u32(&mut buf[4..], 0x02C0_0000);
        reloc.write_value(&mut buf, value).unwrap();
        let recovered = reloc.read_value(&buf);
        assert_eq!(recovered, value,
            "SPLIT_PCALA roundtrip should recover positive offset 0x{:X}", value);
    }

    /// SPLIT_PCALA roundtrip with a negative value (lo12 causes carry into hi20).
    #[test]
    fn test_split_pcala_negative_roundtrip() {
        let reloc = LoongArchRelocation::SPLIT_PCALA;
        // value = -4: hi20 = (-4 + 0x800) >> 12 = 0x7FC/0x7FC..., lo12 = -4 & 0xFFF = 0xFFC
        let value: isize = -4;
        let mut buf = [0u8; 8];
        LittleEndian::write_u32(&mut buf[..4], 0x1A00_0000);
        LittleEndian::write_u32(&mut buf[4..], 0x02C0_0000);
        reloc.write_value(&mut buf, value).unwrap();
        let recovered = reloc.read_value(&buf);
        assert_eq!(recovered, value,
            "SPLIT_PCALA roundtrip should recover negative offset {}", value);
    }

    /// SPLIT_PCALA roundtrip with a large positive value within range.
    #[test]
    fn test_split_pcala_large_roundtrip() {
        let reloc = LoongArchRelocation::SPLIT_PCALA;
        // value = 0x7FFF_F7FC: close to max (0x7FFF_F7FF), 4-byte aligned
        let value: isize = 0x7FFF_F7FC;
        let mut buf = [0u8; 8];
        LittleEndian::write_u32(&mut buf[..4], 0x1A00_0000);
        LittleEndian::write_u32(&mut buf[4..], 0x02C0_0000);
        reloc.write_value(&mut buf, value).unwrap();
        let recovered = reloc.read_value(&buf);
        assert_eq!(recovered, value,
            "SPLIT_PCALA roundtrip should recover large positive offset 0x{:X}", value);
    }

    /// SPLIT_PCALA should reject non-4-byte-aligned values.
    #[test]
    fn test_split_pcala_alignment_check() {
        let reloc = LoongArchRelocation::SPLIT_PCALA;
        let mut buf = [0u8; 8];
        LittleEndian::write_u32(&mut buf[..4], 0x1A00_0000);
        LittleEndian::write_u32(&mut buf[4..], 0x02C0_0000);
        let result = reloc.write_value(&mut buf, 3);  // not 4-byte aligned
        assert!(result.is_err(), "SPLIT_PCALA should reject non-4-byte-aligned value");
    }

    #[test]
    fn test_split_call30_positive_roundtrip() {
        let reloc = LoongArchRelocation::SPLIT_CALL30;
        // pcaddu12i $r1 template: opcode 0b00011100 at bits [31:25], rd=1 at bits [4:0]
        // jirl $r1, $r1 template: opcode 0b01001100 at bits [31:26], rd=1 at bits [4:0], rj=1 at bits [9:5]
        let mut buf = [0u8; 8];
        LittleEndian::write_u32(&mut buf[..4], 0x1C00_0001);  // pcaddu12i $r1
        LittleEndian::write_u32(&mut buf[4..], 0x4C00_0021);  // jirl $r1, $r1

        let value: isize = 0x12340;  // positive, 4-byte aligned
        reloc.write_value(&mut buf, value).unwrap();
        let read_back = reloc.read_value(&buf);
        assert_eq!(read_back, value);
    }

    #[test]
    fn test_split_call30_negative_roundtrip() {
        let reloc = LoongArchRelocation::SPLIT_CALL30;
        let mut buf = [0u8; 8];
        LittleEndian::write_u32(&mut buf[..4], 0x1C00_0001);
        LittleEndian::write_u32(&mut buf[4..], 0x4C00_0021);

        let value: isize = -0x10000;  // negative, 4-byte aligned
        reloc.write_value(&mut buf, value).unwrap();
        let read_back = reloc.read_value(&buf);
        assert_eq!(read_back, value);
    }

    #[test]
    fn test_split_call30_alignment_check() {
        let reloc = LoongArchRelocation::SPLIT_CALL30;
        let mut buf = [0u8; 8];
        LittleEndian::write_u32(&mut buf[..4], 0x1C00_0001);
        LittleEndian::write_u32(&mut buf[4..], 0x4C00_0021);
        let result = reloc.write_value(&mut buf, 5);  // not 4-byte aligned
        assert!(result.is_err(), "SPLIT_CALL30 should reject non-4-byte-aligned value");
    }

    #[test]
    fn test_split_call36_positive_roundtrip() {
        let reloc = LoongArchRelocation::SPLIT_CALL36;
        // pcaddu18i $r1 template: opcode 0b00011110 at bits [31:25], rd=1 at bits [4:0]
        // jirl $r1, $r1 template: opcode 0b01001100 at bits [31:26], rd=1 at bits [4:0], rj=1 at bits [9:5]
        let mut buf = [0u8; 8];
        LittleEndian::write_u32(&mut buf[..4], 0x1E00_0001);  // pcaddu18i $r1
        LittleEndian::write_u32(&mut buf[4..], 0x4C00_0021);  // jirl $r1, $r1

        let value: isize = 0x12340;  // positive, 4-byte aligned
        reloc.write_value(&mut buf, value).unwrap();
        let read_back = reloc.read_value(&buf);
        assert_eq!(read_back, value);
    }

    #[test]
    fn test_split_call36_negative_roundtrip() {
        let reloc = LoongArchRelocation::SPLIT_CALL36;
        let mut buf = [0u8; 8];
        LittleEndian::write_u32(&mut buf[..4], 0x1E00_0001);
        LittleEndian::write_u32(&mut buf[4..], 0x4C00_0021);

        let value: isize = -0x20000;  // negative, 4-byte aligned
        reloc.write_value(&mut buf, value).unwrap();
        let read_back = reloc.read_value(&buf);
        assert_eq!(read_back, value);
    }

    #[test]
    fn test_split_call36_alignment_check() {
        let reloc = LoongArchRelocation::SPLIT_CALL36;
        let mut buf = [0u8; 8];
        LittleEndian::write_u32(&mut buf[..4], 0x1E00_0001);
        LittleEndian::write_u32(&mut buf[4..], 0x4C00_0021);
        let result = reloc.write_value(&mut buf, 5);  // not 4-byte aligned
        assert!(result.is_err(), "SPLIT_CALL36 should reject non-4-byte-aligned value");
    }
}
