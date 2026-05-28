use dynasmrt::{dynasm, DynasmApi, DynasmLabelApi};

use std::io::{BufRead, Write, stdin, stdout, BufReader, BufWriter};
use std::mem;

const SECRET: u8 = 7;

const MESSAGES: [&[u8]; 5] = [
    b"Correct!\n",
    b"Too low!\n",
    b"Too high!\n",
    b"Invalid!\n",
    b"Guess 1-9 (q=quit): ",
];

struct State<'a> {
    input: Box<dyn BufRead + 'a>,
    output: Box<dyn Write + 'a>,
}

impl State<'_> {
    unsafe extern "C" fn read_char(state: *mut State) -> u8 {
        let state = &mut *state;
        let _ = state.output.flush();
        let mut buf = [0u8; 1];
        loop {
            match state.input.read(&mut buf) {
                Ok(0) => return 0,
                Ok(_) => {
                    if buf[0] > b' ' {
                        return buf[0];
                    }
                }
                Err(_) => return 0,
            }
        }
    }

    unsafe extern "C" fn print_msg(state: *mut State, msg_id: u8) -> u8 {
        let state = &mut *state;
        let idx = msg_id as usize;
        if idx >= MESSAGES.len() {
            return 1;
        }
        state.output.write_all(MESSAGES[idx]).is_err() as u8
    }
}

struct Program {
    code: dynasmrt::ExecutableBuffer,
    start: dynasmrt::AssemblyOffset,
}

macro_rules! my_dynasm {
    ($ops:ident $($t:tt)*) => {
        dynasm!($ops
            ; .arch loongarch64
            ; .alias a_state, a0
            ; .alias a_current, a1
            ; .alias retval, a0
            ; .alias a_ret, t2
            $($t)*
        );
    }
}

macro_rules! call_print {
    ($ops:ident, $msg:expr) => {
        my_dynasm!($ops
            ; st.d a_current, sp, 16
            ; addi.d a1, zero, $msg
            ; pcaddi t0, 1
            ; ld.d t0, t0, ->print_msg
            ; jirl ra, t0, 0
            ; add.d a_ret, zero, a0
            ; ld.d a_state, sp, 8
            ; ld.d a_current, sp, 16
        );
    };
}

macro_rules! call_read {
    ($ops:ident) => {
        my_dynasm!($ops
            ; st.d a_current, sp, 16
            ; pcaddi t0, 1
            ; ld.d t0, t0, ->read_char
            ; jirl ra, t0, 0
            ; add.d a_ret, zero, a0
            ; ld.d a_state, sp, 8
            ; add.d a_current, zero, a_ret
        );
    };
}

impl Program {
    fn compile() -> Self {
        let mut ops = dynasmrt::loongarch::Assembler::new().unwrap();

        // Dynamic labels
        let loop_back = ops.new_dynamic_label();
        let quit = ops.new_dynamic_label();

        // Step 1: Emit literal pool (same pattern as bf-jit)
        dynasm!(ops
            ; .arch loongarch64
            ; .align 8
            ; ->read_char:
            ; .u64 State::read_char as *const () as _
            ; ->print_msg:
            ; .u64 State::print_msg as *const () as _
        );

        // Record start of code section
        let start = ops.offset();

        // Step 2: All code in one dynasm block
        dynasm!(ops
            ; .arch loongarch64
            ; .alias a_state, a0
            ; .alias a_current, a1
            ; .alias retval, a0
            ; .alias a_ret, t2

            // Prologue: 24 bytes for ra, state, a_current
            ; addi.d sp, sp, -24
            ; st.d ra, sp, 0
            ; st.d a_state, sp, 8

            // Print prompt (msg_id=4)
            ;; call_print!(ops, 4)

            // === Loop entry ===
            ; =>loop_back

            // Read char
            ;; call_read!(ops)

            // EOF check
            ; beqz a_current, ->io_failure

            // Quit check: 'q' == 113
            ; addi.d t1, zero, 113
            ; beq a_current, t1, =>quit

            // Range check: '0' = 49, '9' = 57
            ; addi.d t1, zero, 49
            ; blt a_current, t1, >invalid
            ; addi.d t1, zero, 58
            ; bge a_current, t1, >invalid

            // Valid digit: convert ASCII to number
            ; addi.d a_current, a_current, -48

            // Compare with SECRET
            ; addi.d t1, zero, SECRET as i32
            ; beq a_current, t1, >correct
            ; blt a_current, t1, >too_low
            ; b >too_high

            // === >correct: print msg 0 ===
            ; correct:
            ;; call_print!(ops, 0)
            ; beqz a_ret, =>quit
            ; b ->io_failure

            // === >too_low: print msg 1 ===
            ; too_low:
            ;; call_print!(ops, 1)
            ; bnez a_ret, ->io_failure
            ; b =>loop_back

            // === >too_high: print msg 2 ===
            ; too_high:
            ;; call_print!(ops, 2)
            ; bnez a_ret, ->io_failure
            ; b =>loop_back

            // === >invalid: print msg 3 ===
            ; invalid:
            ;; call_print!(ops, 3)
            ; bnez a_ret, ->io_failure
            ; b =>loop_back

            // === Quit ===
            ; =>quit
            ; addi.d retval, zero, 0
            ; ld.d ra, sp, 0
            ; addi.d sp, sp, 24
            ; jirl zero, ra, 0

            // === IO failure ===
            ; ->io_failure:
            ; addi.d retval, zero, 2
            ; ld.d ra, sp, 0
            ; addi.d sp, sp, 24
            ; jirl zero, ra, 0
        );

        let buf = ops.finalize().unwrap();
        Program { code: buf, start }
    }

    fn run(self, state: &mut State) -> Result<(), &'static str> {
        let f: extern "C" fn(*mut State) -> u8 =
            unsafe { mem::transmute(self.code.ptr(self.start)) };
        let res = f(state);
        match res {
            0 => Ok(()),
            2 => Err("I/O error"),
            _ => Err("unexpected return value"),
        }
    }
}

fn main() {
    let mut state = State {
        input: Box::new(BufReader::new(stdin())),
        output: Box::new(BufWriter::new(stdout())),
    };

    let program = Program::compile();
    match program.run(&mut state) {
        Ok(()) => {}
        Err(e) => eprintln!("Error: {}", e),
    }
}