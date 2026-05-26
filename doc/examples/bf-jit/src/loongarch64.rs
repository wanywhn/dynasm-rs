use dynasmrt::{dynasm, DynasmApi, DynasmLabelApi};

use itertools::Itertools;
use itertools::multipeek;

use std::io::{Read, BufRead, Write, stdin, stdout, BufReader, BufWriter};
use std::env;
use std::fs::File;
use std::slice;
use std::mem;
use std::u8;

const TAPE_SIZE: usize = 30000;
macro_rules! my_dynasm {
    ($ops:ident $($t:tt)*) => {
        dynasm!($ops
            ; .arch loongarch64
            ; .alias a_state, a0
            ; .alias a_current, a1
            ; .alias a_begin, a2
            ; .alias a_end, a3
            ; .alias retval, a0
            $($t)*
        );
    }
}

macro_rules! prologue {
    ($ops:ident) => {{
        let start = $ops.offset();
        my_dynasm!($ops
            ; addi.d sp, sp, -48
            ; st.d ra, sp, 0
            ; st.d a0, sp, 8
            ; st.d a1, sp, 16
            ; st.d a2, sp, 24
            ; st.d a3, sp, 32
        );
        start
    }};
}

macro_rules! epilogue {
    ($ops:ident, $e:expr) => {my_dynasm!($ops
        ; addi.d a0, zero, $e
        ; ld.d ra, sp, 0
        ; addi.d sp, sp, 48
        ; jirl zero, ra, 0
    );};
}

macro_rules! call_extern {
    ($ops:ident, $addr:ident) => {my_dynasm!($ops
        ; st.d a1, sp, 16
        ; pcaddi a4, 1
        ; ld.d a4, a4, ->$addr
        ; jirl ra, a4, 0
        ; add.d a4, zero, a0
        ; ld.d a0, sp, 8
        ; ld.d a1, sp, 16
        ; ld.d a2, sp, 24
        ; ld.d a3, sp, 32
    );};
}

#[macro_export]
#[doc(hidden)]
macro_rules! add_imm {
    ($ops:ident, $rd:expr, $rs:expr, $imm:expr, $rt:expr) => {{
        // 处理12位有符号数范围 (-2048 到 2047)
        if $imm >= -2048 && $imm <= 2047 {
            my_dynasm!($ops
                ; addi.d $rd, $rs, ($imm as i64).try_into().unwrap()
            );
        }
        // 处理16位有符号数且可以被16整除的情况
        else if ($imm >= -(1 << 15) && $imm < (1 << 15)) && ($imm & 0xFFFF == 0) {
            let si16 = ($imm >> 16) as i16;
            my_dynasm!($ops
                ; addu16i.d $rd, $rs, si16 as i32
            );
        }
        // 处理20位有符号数范围
        else if $imm >= -(1 << 19) && $imm < (1 << 19) {
            let si20 = ($imm >> 12) as i32;
            let low12 = ($imm & 0xFFF) as u32;
            my_dynasm!($ops
                ; lu12i.w $rt, si20
                ; ori $rt, $rt, low12
                ; add.d $rd, $rs, $rt
            );
        }
        // 处理32位数
        // lu12i.w: SImm(5,20) 有符号20位，范围[-524288, 524287]
        // 需将 (imm>>12)&0xFFFFF 从无符号20位符号扩展为i32
        else if $imm >= -(1i64 << 31) && $imm < (1i64 << 31) {
            let low12 = ($imm & 0xFFF) as u32;
            let si20 = {
                let raw = (($imm >> 12) & 0xFFFFF) as u32;
                if raw & 0x80000 != 0 { (raw as i32) - 0x100000 } else { raw as i32 }
            };
            my_dynasm!($ops
                ; lu12i.w $rt, si20
                ; ori $rt, $rt, low12
                ; add.d $rd, $rs, $rt
            );
        }
        // 处理52位数
        // lu12i.w/lu32i.d: SImm(5,20) 需符号扩展
        // lu52i.d: SImm(52,12) 有符号12位，范围[-2048, 2047]
        // 需将各字段从无符号截取值符号扩展为有符号i32
        else {
            let low12 = ($imm & 0xFFF) as u32;
            let si20_low = {
                let raw = (($imm >> 12) & 0xFFFFF) as u32;
                if raw & 0x80000 != 0 { (raw as i32) - 0x100000 } else { raw as i32 }
            };
            let si20_high = {
                let raw = (($imm >> 32) & 0xFFFFF) as u32;
                if raw & 0x80000 != 0 { (raw as i32) - 0x100000 } else { raw as i32 }
            };
            let si12_top = {
                let raw = (($imm >> 52) & 0xFFF) as u32;
                if raw & 0x800 != 0 { (raw as i32) - 0x1000 } else { raw as i32 }
            };
            my_dynasm!($ops
                ; lu12i.w $rt, si20_high
                ; ori $rt, $rt, low12
                ; lu32i.d $rt, si20_low
                ; lu52i.d $rt, $rt, si12_top
                ; add.d $rd, $rs, $rt
            );
        }
    }};
}

#[macro_export]
#[doc(hidden)]
macro_rules! sub_imm {
    ($ops:ident, $rd:expr, $rs:expr, $imm:expr, $rt:expr) => {{
        add_imm!($ops, $rd, $rs, - (($imm) as i64), $rt)
    }};
}

#[cfg(test)]
mod tests {
    use dynasmrt::{dynasm, DynasmApi};
    use dynasmrt::loongarch::Assembler;

    #[test]
    fn test_add_imm() {
        let mut ops = Assembler::new().unwrap();
        
        // 测试12位范围内的数
        add_imm!(ops, a0, t0, 2047, t1);
        add_imm!(ops, a0, t0, -2048, t1);
        
        // 测试16位且可左移16位的数
        add_imm!(ops, a0, t0, 0x10000, t1);
        
        // 测试20位数
        add_imm!(ops, a0, t0, 0x7FFFF, t1);
        
        // 测试32位数
        add_imm!(ops, a0, t0, 0x7FFFFFFF, t1);
        
        // 测试52位数
        add_imm!(ops, a0, t0, 0xFFFFFFFFFFFFFi64, t1);
    }

    #[test]
    fn test_sub_imm() {
        let mut ops = Assembler::new().unwrap();
        
        // 测试12位范围内的数
        sub_imm!(ops, a0, t0, 2047, t1);
        sub_imm!(ops, a0, t0, -2048, t1);
        
        // 测试16位且可左移16位的数
        sub_imm!(ops, a0, t0, 0x10000, t1);
        
        // 测试20位数
        sub_imm!(ops, a0, t0, 0x7FFFF, t1);
        
        // 测试32位数
        sub_imm!(ops, a0, t0, 0x7FFFFFFF, t1);
        
        // 测试52位数
        sub_imm!(ops, a0, t0, 0xFFFFFFFFFFFFFi64, t1);
    }
}

struct State<'a> {
    pub input: Box<dyn BufRead + 'a>,
    pub output: Box<dyn Write + 'a>,
    tape: [u8; TAPE_SIZE],
}

struct Program {
    code: dynasmrt::ExecutableBuffer,
    start: dynasmrt::AssemblyOffset,
}


impl Program {
    fn compile(program: &[u8]) -> Result<Program, &'static str> {
        let mut ops = dynasmrt::loongarch::Assembler::new().unwrap();
        let mut loops = Vec::new();
        let mut code = multipeek(program.iter().cloned());

        // literal pool
        dynasm!(ops
            ; .align 8
            ; ->getchar:
            ; .u64 State::getchar as _
            ; ->putchar:
            ; .u64 State::putchar as _
        );

        let start = prologue!(ops);

        while let Some(c) = code.next() {
            match c {
                b'<' => {
                    let amount = code.take_while_ref(|x| *x == b'<').count() + 1;
                    sub_imm!(ops, a_current, a_current, (amount % TAPE_SIZE) as i64, a4);
                    my_dynasm!(ops
                        ; bgeu a_current, a_begin, >nowrap
                    );
                    add_imm!(ops, a_current, a_current, TAPE_SIZE as i64, a4);
                    my_dynasm!(ops
                        ; nowrap:
                    );
                },
                b'>' => {
                    let amount = code.take_while_ref(|x| *x == b'>').count() + 1;
                    add_imm!(ops, a_current, a_current, (amount % TAPE_SIZE) as i64, a4);
                    my_dynasm!(ops
                        ; bltu a_current, a_end, >nowrap
                    );
                    sub_imm!(ops, a_current, a_current, TAPE_SIZE as i64, a4);
                    my_dynasm!(ops
                        ; nowrap:
                    );
                },
                b'+' => {
                    let amount = code.take_while_ref(|x| *x == b'+').count() + 1;
                    if amount > u8::MAX as usize {
                        return Err("An overflow occurred");
                    }
                    my_dynasm!(ops
                       ; ld.b a4, a_current, 0
                        ; addi.d a4, a4, amount as i32
                        ; addi.d a5, a4, -256
                        ; blt a5, zero, >fine
                        ; b ->overflow
                        ; fine:
                        ; st.b a4, a_current, 0
                    );
                },
                b'-' => {
                    let amount = code.take_while_ref(|x| *x == b'-').count() + 1;
                    if amount > u8::MAX as usize {
                        return Err("An overflow occurred");
                    }
                    my_dynasm!(ops
                        ; ld.b a4, a_current, 0
                        ; addi.d a4, a4, -(amount as i32)
                        ; bge a4, zero, >fine
                        ; b ->overflow
                        ; fine:
                        ; st.b a4, a_current, 0
                    );
                },
                b',' => {
                    my_dynasm!(ops
                        ;; call_extern!(ops, getchar)
                        ; beqz a4, >fine
                        ; b ->io_failure
                        ; fine:
                    );
                },
                b'.' => {
                    my_dynasm!(ops
                        ;; call_extern!(ops, putchar)
                        ; beqz a4, >fine
                        ;  b ->io_failure
                        ; fine:
                    );
                },
                b'[' => {
                    let first = code.peek() == Some(&b'-');
                    if first && code.peek() == Some(&b']') {
                        code.next();
                        code.next();
                        my_dynasm!(ops
                            ; st.b zero, a_current, 0
                        );
                    } else {
                        let backward_label = ops.new_dynamic_label();
                        let forward_label = ops.new_dynamic_label();
                        loops.push((backward_label, forward_label));
                        my_dynasm!(ops
                            ; ld.b a4, a_current, 0
                            ; bnez a4, =>backward_label
                            ; b =>forward_label
                            ;=>backward_label
                        );
                    }
                },
                b']' => {
                    if let Some((backward_label, forward_label)) = loops.pop() {
                        my_dynasm!(ops
                            ; ld.b a4, a_current, 0
                            ; beqz a4, =>forward_label
                            ; b =>backward_label
                            ;=>forward_label
                        );
                    } else {
                        return Err("] without matching [");
                    }
                },
                _ => (),
            }
        }
        if loops.len() != 0 {
            return Err("[ without matching ]");
        }

        my_dynasm!(ops
            ;; epilogue!(ops, 0)
            ;->overflow:
            ;; epilogue!(ops, 1)
            ;->io_failure:
            ;; epilogue!(ops, 2)
        );

        let code = ops.finalize().unwrap();
        Ok(Program {
            code: code,
            start: start,
        })
    }

    fn run(self, state: &mut State) -> Result<(), &'static str> {
        let f: extern "C" fn(*mut State, *mut u8, *mut u8, *const u8) -> u8 =
            unsafe { mem::transmute(self.code.ptr(self.start)) };
        let start = state.tape.as_mut_ptr();
        let end = unsafe { start.offset(TAPE_SIZE as isize) };
        let res = f(state, start, start, end);
        if res == 0 {
            Ok(())
        } else if res == 1 {
            Err("An overflow occurred")
        } else if res == 2 {
            Err("IO error")
        } else {
            panic!("Unknown error code");
        }
    }
}

impl<'a> State<'a> {
    unsafe extern "C" fn getchar(state: *mut State, cell: *mut u8) -> u8 {
        let state = &mut *state;
        let err = state.output.flush().is_err();
        (state.input.read_exact(slice::from_raw_parts_mut(cell, 1)).is_err() || err) as u8
    }

    unsafe extern "C" fn putchar(state: *mut State, cell: *mut u8) -> u8 {
        let state = &mut *state;
        state.output.write_all(slice::from_raw_parts(cell, 1)).is_err() as u8
    }

    fn new(input: Box<dyn BufRead + 'a>, output: Box<dyn Write + 'a>) -> State<'a> {
        State {
            input: input,
            output: output,
            tape: [0; TAPE_SIZE],
        }
    }
}


fn main() {
    let mut args: Vec<_> = env::args().collect();
    if args.len() != 2 {
        println!("Expected 2 argument, got {}", args.len());
        return;
    }
    let path = args.pop().unwrap();

    let mut f = if let Ok(f) = File::open(&path) {
        f
    } else {
        println!("Could not open file {}", path);
        return;
    };

    let mut buf = Vec::new();
    if let Err(_) = f.read_to_end(&mut buf) {
        println!("Failed t0 read from file");
        return;
    }

    let mut state = State::new(Box::new(BufReader::new(stdin())),
                               Box::new(BufWriter::new(stdout())));
    let program = match Program::compile(&buf) {
        Ok(p) => p,
        Err(e) => {
            println!("{}", e);
            return;
        }
    };
    if let Err(e) = program.run(&mut state) {
        println!("{}", e);
        return;
    }
}