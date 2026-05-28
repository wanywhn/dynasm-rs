// Test la pseudo-instruction with label and immediate
#![allow(unused_imports)]

use dynasmrt::dynasm;
use dynasmrt::DynasmApi;
use dynasmrt::DynasmLabelApi;

#[test]
fn test_la_immediate() {
    let mut ops = dynasmrt::loongarch::Assembler::new().unwrap();

    dynasm!(ops
        ; .arch loongarch64
        ; la r10, 0x12340
    );

    let buf: Vec<u8> = ops.finalize().unwrap().iter().copied().collect();
    assert_eq!(buf.len(), 8, "Expected 8 bytes (2 instructions)");

    let insn1 = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    let insn2 = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);

    // pcalau12i r10, hi20: opcode[31:26]=000110
    assert_eq!((insn1 >> 26) & 0x3F, 0b000110, "pcalau12i opcode");
    assert_eq!(insn1 & 0x1F, 10, "pcalau12i rd = r10");

    // addi.d r10, r10, lo12: opcode[31:26]=000000, func[25:22]=1011
    assert_eq!((insn2 >> 26) & 0x3F, 0b000000, "addi.d opcode");
    assert_eq!((insn2 >> 22) & 0xF, 0b1011, "addi.d func field");
    assert_eq!(insn2 & 0x1F, 10, "addi.d rd = r10");
    assert_eq!((insn2 >> 5) & 0x1F, 10, "addi.d rj = r10");
}

#[test]
fn test_la_label_backward() {
    let mut ops = dynasmrt::loongarch::Assembler::new().unwrap();

    dynasm!(ops
        ; .arch loongarch64
        ; start:
        ; la r5, <start
        ; b <start
    );

    let buf: Vec<u8> = ops.finalize().unwrap().iter().copied().collect();
    assert_eq!(buf.len(), 12, "Expected 12 bytes (la=8 + b=4)");

    // First instruction should be pcalau12i (opcode 0b000110)
    let insn1 = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    assert_eq!(
        (insn1 >> 26) & 0x3F,
        0b000110,
        "First instruction should be pcalau12i"
    );
    assert_eq!(insn1 & 0x1F, 5, "pcalau12i rd should be r5");

    // Second instruction should be addi.d (opcode 0b000000, func 0b1011)
    let insn2 = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
    assert_eq!(
        (insn2 >> 26) & 0x3F,
        0b000000,
        "Second instruction should be addi.d"
    );
    assert_eq!((insn2 >> 22) & 0xF, 0b1011, "addi.d func field should be 0b1011");
    assert_eq!(insn2 & 0x1F, 5, "addi.d rd should be r5");
    assert_eq!((insn2 >> 5) & 0x1F, 5, "addi.d rj should be r5");
}
