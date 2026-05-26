//! Temporary test for new [base, offset] and offset(base) memory reference syntax

#![allow(unused_imports)]

use dynasmrt::dynasm;
use dynasmrt::DynasmApi;
use dynasmrt::loongarch::Assembler;

// Known test case: fld.d f14, r2, -403 encodes to 4E, B4, B9, 2B
// We test this with the new syntax variants

#[test]
fn test_fld_d_bracket_offset() {
    // Test [base, offset] syntax
    let mut ops = Assembler::new().unwrap();
    dynasm!(ops
        ; .arch loongarch64
        ; fld.d f14, [r2, -403]
    );
    let buf = ops.finalize().unwrap();
    let hex: Vec<String> = buf.iter().map(|x| format!("{:02X}", *x)).collect();
    let hex = hex.join(", ");
    assert_eq!(hex, "4E, B4, B9, 2B", "fld.d f14, [r2, -403] via [base, offset]");
}

#[test]
fn test_fld_d_gas_syntax() {
    // Test offset(base) GAS syntax
    let mut ops = Assembler::new().unwrap();
    dynasm!(ops
        ; .arch loongarch64
        ; fld.d f14, -403(r2)
    );
    let buf = ops.finalize().unwrap();
    let hex: Vec<String> = buf.iter().map(|x| format!("{:02X}", *x)).collect();
    let hex = hex.join(", ");
    assert_eq!(hex, "4E, B4, B9, 2B", "fld.d f14, -403(r2) via offset(base)");
}

#[test]
fn test_fld_d_original_syntax() {
    // Test original bare-args syntax (must still work)
    let mut ops = Assembler::new().unwrap();
    dynasm!(ops
        ; .arch loongarch64
        ; fld.d f14, r2, -403
    );
    let buf = ops.finalize().unwrap();
    let hex: Vec<String> = buf.iter().map(|x| format!("{:02X}", *x)).collect();
    let hex = hex.join(", ");
    assert_eq!(hex, "4E, B4, B9, 2B", "fld.d f14, r2, -403 via original bare-args");
}

#[test]
fn test_ld_d_bracket_offset() {
    // ld.d r4, r3, 16 -> expected encoding
    // 00101000_11_r4_r3_0000000000_0100 (16 = 0x010 in SI12)
    let mut ops = Assembler::new().unwrap();
    dynasm!(ops
        ; .arch loongarch64
        ; ld.d r4, [r3, 16]
    );
    let buf = ops.finalize().unwrap();
    let hex: Vec<String> = buf.iter().map(|x| format!("{:02X}", *x)).collect();
    let hex = hex.join(", ");
    assert_eq!(hex, "64, 40, C0, 28", "ld.d r4, [r3, 16] via [base, offset]");
}

#[test]
fn test_ld_d_gas_syntax() {
    let mut ops = Assembler::new().unwrap();
    dynasm!(ops
        ; .arch loongarch64
        ; ld.d r4, 16(r3)
    );
    let buf = ops.finalize().unwrap();
    let hex: Vec<String> = buf.iter().map(|x| format!("{:02X}", *x)).collect();
    let hex = hex.join(", ");
    assert_eq!(hex, "64, 40, C0, 28", "ld.d r4, 16(r3) via offset(base)");
}

#[test]
fn test_st_w_bracket_offset() {
    let mut ops = Assembler::new().unwrap();
    dynasm!(ops
        ; .arch loongarch64
        ; st.w r5, [r2, 100]
    );
    let buf = ops.finalize().unwrap();
    let hex: Vec<String> = buf.iter().map(|x| format!("{:02X}", *x)).collect();
    let hex = hex.join(", ");
    assert_eq!(hex, "45, 90, 81, 29", "st.w r5, [r2, 100] via [base, offset]");
}

#[test]
fn test_st_w_gas_syntax() {
    let mut ops = Assembler::new().unwrap();
    dynasm!(ops
        ; .arch loongarch64
        ; st.w r5, 100(r2)
    );
    let buf = ops.finalize().unwrap();
    let hex: Vec<String> = buf.iter().map(|x| format!("{:02X}", *x)).collect();
    let hex = hex.join(", ");
    assert_eq!(hex, "45, 90, 81, 29", "st.w r5, 100(r2) via offset(base)");
}
