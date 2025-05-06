use dynasmrt::{dynasm, DynasmApi, DynasmLabelApi};

use std::{io, slice, mem};
use std::io::Write;

fn main() {
    let mut ops = dynasmrt::loongarch::Assembler::new().unwrap();
    let string = "Hello World!";

    dynasm!(ops
        ; .arch loongarch64
        ; ->hello:
        ; .bytes string.as_bytes()
        ; .align 8
        ; ->print:
        ; .u64 print as _
    );

    let hello = ops.offset();
    dynasm!(ops
        ; .arch loongarch64
        ; addi.d sp, sp, -16
        ; st.d ra, sp, 0
        ; pcaddi a0, ->hello
        ; addi.d a1, zero, string.len() as i32
        ; pcaddi t0, ->print
        ; ldptr.d t0, t0, 0
        // ; ld.d t0, zero, ->print
        ; jirl ra, t0, 0
        ; ld.d ra, sp, 0
        ; addi.d sp, sp, 16
        ; jirl zero, ra, 0
    );

    let buf = ops.finalize().unwrap();

    let hello_fn: extern "C" fn() -> bool = unsafe { mem::transmute(buf.ptr(hello)) };

    assert!(hello_fn());
}

pub extern "C" fn print(buffer: *const u8, length: u64) -> bool {
    io::stdout()
        .write_all(unsafe { slice::from_raw_parts(buffer, length as usize) })
        .is_ok()
}
