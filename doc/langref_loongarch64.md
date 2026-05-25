% loongarch64 assembly language reference

# Lexical structure definition

Instructions for the `loongarch64` assembling backend use the following lexical structure:

## Base units

The following base syntax units are recognized by the parser.

- `static_reg_name` matches any valid register name as seen in table 2, or any previously defined alias
- `dynamic_reg_family` matches any valid register family from table 2

## Instruction

`instruction : ident ("." ident)* (arg ("," arg)* )? ;`

## Arguments

`arg : register | labelref | reference | immediate ;`

`register : scalar_reg | vector_reg ;`

`scalar_reg : static_reg_name | dynamic_reg_family "(" expr ")"`

`vector_reg : vector_reg_name | "V" "(" expr ")" | "X" "(" expr ")" ;`

`reference : "[" register ("," immediate | labelref)? "]" ;`

`immediate : "#"? expr ;`

# Reference

## Targets

The LoongArch instruction set family currently has one architecture target supported by dynasm-rs. It can be selected using the `.arch` directive:

Table 1: dynasm-rs LoongArch architecture support

Instruction set | Directive          | Integer register width | Integer register count
---------------:|:-------------------|:-----------------------|:-----------------------
`LA64`          | `.arch loongarch64`| `64`                   | `32`

Note: dynasm-rs currently only supports the 64-bit LoongArch target (`loongarch64`). The 32-bit LA32 target is not yet implemented.

## Instructions

The assembly language used by dynasm-rs in loongarch64 mode is inspired by the assembly dialect used by the GNU assembler for LoongArch. Several additions have been made to support dynamic registers, and to ensure the Rust parser can parse it.

A significant difference exists in the syntax used for memory references. The GNU assembler uses `offset(base_register)` syntax for these. Use of this syntax in dynasm-rs would cause parsing ambiguities as it is unclear if the given expression should be parsed as an immediate that contains a function call, or a memory reference. Therefore, the dynasm-rs LoongArch assembly language uses `[base, offset]` memory references, similar to the dynasm-rs RISC-V dialect.

### Operands

#### Register

There are two ways to reference registers in dynasm-rs, either via their static name, or via dynamic register references. Dynamic register references allow the exact register choice to be made at runtime. Note that this does prevent optimizations to register-specific forms. However, the expression inside a dynamic register reference may be evaluated multiple times.

The following table lists all available static registers, their dynamic family name and their encoding when they are used dynamically.

Table 2: dynasm-rs registers (loongarch64)

Family            | 64-bit      | 64-bit       | FP        | FP         | FP          | Condition | Condition
-----------------:|:------------|:-------------|:----------|:-----------|:------------|:----------|:----------
Dynamic Encoding  | `R`         | `F`          |           |            |             | `C`       |
              `0` | `r0/zero`   | `f0/fa0`     |           |            |             | `fcc0`    |
              `1` | `r1/ra`     | `f1/fa1`     |           |            |             | `fcc1`    |
              `2` | `r2/tp`     | `f2/fa2`     |           |            |             | `fcc2`    |
              `3` | `r3/sp`     | `f3/fa3`     |           |            |             | `fcc3`    |
              `4` | `r4/a0`     | `f4/fa4`     |           |            |             | `fcc4`    |
              `5` | `r5/a1`     | `f5/fa5`     |           |            |             | `fcc5`    |
              `6` | `r6/a2`     | `f6/fa6`     |           |            |             | `fcc6`    |
              `7` | `r7/a3`     | `f7/fa7`     |           |            |             | `fcc7`    |
              `8` | `r8/a4`     | `f8/ft0`     |           |            |             |           |
              `9` | `r9/a5`     | `f9/ft1`     |           |            |             |           |
             `10` | `r10/a6`    | `f10/ft2`    |           |            |             |           |
             `11` | `r11/a7`    | `f11/ft3`    |           |            |             |           |
             `12` | `r12/t0`    | `f12/ft4`    |           |            |             |           |
             `13` | `r13/t1`    | `f13/ft5`    |           |            |             |           |
             `14` | `r14/t2`    | `f14/ft6`    |           |            |             |           |
             `15` | `r15/t3`    | `f15/ft7`    |           |            |             |           |
             `16` | `r16/t4`    | `f16/ft8`    |           |            |             |           |
             `17` | `r17/t5`    | `f17/ft9`    |           |            |             |           |
             `18` | `r18/t6`    | `f18/ft10`   |           |            |             |           |
             `19` | `r19/t7`    | `f19/ft11`   |           |            |             |           |
             `20` | `r20/t8`    | `f20/ft12`   |           |            |             |           |
             `21` | `r21/u0`    | `f21/ft13`   |           |            |             |           |
             `22` | `r22/fp`    | `f22/ft14`   |           |            |             |           |
             `23` | `r23/s0`    | `f23/ft15`   |           |            |             |           |
             `24` | `r24/s1`    | `f24/fs0`    |           |            |             |           |
             `25` | `r25/s2`    | `f25/fs1`    |           |            |             |           |
             `26` | `r26/s3`    | `f26/fs2`    |           |            |             |           |
             `27` | `r27/s4`    | `f27/fs3`    |           |            |             |           |
             `28` | `r28/s5`    | `f28/fs4`    |           |            |             |           |
             `29` | `r29/s6`    | `f29/fs5`    |           |            |             |           |
             `30` | `r30/s7`    | `f30/fs6`    |           |            |             |           |
             `31` | `r31/s8`    | `f31/fs7`    |           |            |             |           |

Family            | LSX vector  | LASX vector | Status    | Status
-----------------:|:------------|:------------|:----------|:----------
Dynamic Encoding  | `V`         | `X`         |           |
              `0` | `v0`        | `x0`        | `fcsr0`   |
              `1` | `v1`        | `x1`        | `fcsr1`   |
              `2` | `v2`        | `x2`        | `fcsr2`   |
              `3` | `v3`        | `x3`        | `fcsr3`   |
              `4` | `v4`        | `x4`        |           |
              `5` | `v5`        | `x5`        |           |
             ...  | ...         | ...         |           |
             `31` | `v31`       | `x31`       |           |

When used statically, the notation simply matches the given name in the table. When used dynamically, the syntax is similar to a function call: `R(reg_number)`, `F(reg_number)`, `V(reg_number)`, or `X(reg_number)`, where `reg_number` is one of the given dynamic encodings listed in the table.
Note the `reg_number` can be of an arbitrary type that implements `Into<u8>`.

#### Register families

LoongArch64 has the following register families, each occupying a distinct register bank with no overlap (except LSX/LASX vector registers which physically share the same storage):

Family         | Registers   | Width     | Count | Encoding size | Scope
:--------------|:------------|:----------|:------|:--------------|:-----
GR (General)   | `r0`-`r31`  | 64-bit    | 32    | 5-bit         | Base ISA
FR (Floating)  | `f0`-`f31`  | 64-bit    | 32    | 5-bit         | FP extensions
FCC (Condition)| `fcc0`-`fcc7`| 1-bit     | 8     | 3-bit         | FP extensions
FCSR (Status)  | `fcsr0`-`fcsr3`| 32-bit   | 4     | immediate     | FP extensions
LSX VR (Vector)| `v0`-`v31`  | 128-bit   | 32    | 5-bit         | LSX extension
LASX XR (Vector)| `x0`-`x31`  | 256-bit   | 32    | 5-bit         | LASX extension

Key notes:

- `r0` is the zero register: it always reads as zero and writes are discarded. This is similar to RISC-V's `x0/zero`.
- `r3/sp` is the stack pointer. Unlike ARM, there is no dedicated SP register encoding — `sp` is simply an alias for `r3` and uses the same GR encoding as any other general register.
- `r22/fp` is the frame pointer by convention, but is a general-purpose register.
- FCC registers are 1-bit condition flags, exclusively for FP comparison results. LoongArch has no integer condition code register.
- FCSR registers are accessed via dedicated instructions (`movgr2fcsr`/`movfcsr2gr`) with the FCSR number encoded as an unsigned immediate, not as a register slot.
- LSX and LASX vector registers physically overlap: `x0` is the 256-bit register, and `v0` is the low 128-bit half of the same physical register. Writing to `v0` modifies the lower half of `x0`.
- Moving data between register families requires explicit transfer instructions: `movgr2fr`/`movfr2gr` (GR↔FR), `movgr2fcc`/`movfcc2gr` (GR↔FCC), `movfr2fcc`/`movfcc2fr` (FR↔FCC), `movgr2scr`/`movscr2gr` (GR↔LBT scratch).

#### Jump targets

All flow control instructions and instructions featuring PC-relative addressing have a jump target as argument. This jump target will feature a label reference as described in the common language reference. Note that this reference must be encoded in a limited amount of bits due to the fixed-width 32-bit loongarch64 instruction set, so check the instruction reference to see what the maximum offset range is.

#### Memory references

As a load-store architecture, the loongarch64 instruction set only has a limited amount of instructions capable of addressing memory. Furthermore, it supports a limited set of addressing modes. The available addressing modes for each instruction are listed directly in the instruction reference. All possible addressing modes are summarized in the table below as well.

Table 3: dynasm-rs LoongArch memory reference formats

Syntax                           | Explanation
:--------------------------------|:-----------
<code>[Rn]</code>                | An `R` family register is used as the address to be resolved.
<code>[Rn, imm]</code>           | An `R` family register is used as base with a signed integer offset as the address to be resolved.
<code>[Rn, labelref]</code>      | The lower 12 bits of a relocation are added to an address in the `R` family register. See the section on pc-relative instructions for further details.

Note: Unlike ARM, LoongArch does not have a separate stack pointer register encoding. `sp` is simply `r3`, and it can be used in memory references just like any other `R` family register.

Examples:

- `ld.d r4, [r3, 16]` — load a 64-bit word from `sp + 16`
- `st.w r5, [r12]` — store a 32-bit word to address in `r12`
- `ldx.d r4, r12, r5` — load a 64-bit word from `r12 + r5` (indexed, no brackets)

#### Immediates

The LoongArch64 instruction set features both signed and unsigned immediate operands. The size of these immediates varies per instruction format, and is often not a clean power-of-two. Dynasm-rs expects the type of dynamic LoongArch immediates to be `i32` for signed immediates and `u32` for unsigned immediates. These immediates are where possible validated at compile time. If an impossible immediate is provided at runtime, this will result in a panic.

Several instructions have additional requirements on any passed immediates. Consult the instruction reference for the exact requirements of each instruction.

### Instruction Set Extensions

The LoongArch instruction set has a base instruction set and defines a set of optional extensions. Selecting the active set of instruction set extensions in dynasm-rs is done using the `.feature` directive.

Table 4: LoongArch instruction set extensions

Extension      | Description                          | Scope
:--------------|:-------------------------------------|:----------
Base           | Core integer arithmetic, logic, shifts, branches, loads/stores | Mandatory
FP-S           | Single-precision floating-point      | Optional
FP-D           | Double-precision floating-point      | Optional
FP Control     | FCSR read/write, FCC move, `fsel`, `bceqz/bcnez` | With FP
Atomics        | LL/SC, AMO (atomic memory operations)| Optional
Bit Ops        | Count/reverse/bit-string operations  | Part of base
Multiply       | Integer multiply/divide              | Optional
Bound Check    | Bounds-checked loads/stores          | Optional
LSX            | LoongArch SIMD Extension (128-bit)   | Optional
LASX           | LoongArch Advanced SIMD Extension (256-bit) | Optional, extends LSX
LBT            | Binary Translation assist (x86/ARM emulation) | Optional
LVZ            | Virtualization (hypervisor)          | Optional, kernel only
Privileged     | CSR, IOCSR, TLB management, cache ops| Kernel only

Extension dependency graph:

```
Base (mandatory)
  ├── FP-S (optional)
  │   └── FP-D (optional)
  │       └── FP Control (with any FP)
  ├── Atomics (optional)
  ├── Multiply (optional)
  ├── Bound Check (optional)
  ├── LSX (optional, 128-bit SIMD)
  │   └── LASX (optional, 256-bit SIMD, extends LSX)
  ├── LBT (optional, binary translation)
  ├── LVZ (optional, kernel only)
  └── Privileged (kernel only)
```

### Branch and jump instructions

LoongArch64 has a layered branch/jump system with different offset ranges:

Table 5: LoongArch64 branch/jump offset ranges

Instructions                        | Offset bits | Alignment | Range
:-----------------------------------|:------------|:----------|:-----------------------------------
`b`, `bl`                           | 26          | 4-byte    | `pc - 0x8000000` to `pc + 0x7FFFFFC`
`beqz`, `bnez`                      | 21          | 4-byte    | `pc - 0x400000` to `pc + 0x3FFFFC`
`bceqz`, `bcnez`                    | 21          | 4-byte    | `pc - 0x400000` to `pc + 0x3FFFFC`
`beq`, `bne`, `blt`, `bge`, `bltu`, `bgeu` | 16 | 4-byte    | `pc - 0x80000` to `pc + 0x7FFFC`
`jirl`                              | 16          | 4-byte    | `pc - 0x80000` to `pc + 0x7FFFC`

All branch offsets are PC-relative, signed, and require 4-byte alignment (shifted left by 2 in the encoding).

### PC-relative instructions

LoongArch64 provides several instructions for PC-relative address computation, similar to RISC-V's `auipc` mechanism but with a different encoding structure:

Table 6: LoongArch64 PC-relative address generation instructions

Instruction     | Offset bits | Function
:---------------|:------------|:-----------------------------------
`pcaddu2i`      | 20 + 2      | `rd = pc + (si20 << 2)`
`pcaddu12i`     | 20 + 12     | `rd = pc + (si20 << 12)`
`pcaddu18i`     | 20 + 18     | `rd = pc + (si20 << 18)`
`pcalau12i`     | 20 + 12     | `rd = pc + (si20 << 12)` (upper, for combining with lower 12-bit offsets)
`lu12i.w`       | 20          | `rd = si20 << 12` (non-PC-relative, constant generation)
`lu32i.d`       | 20          | `rd[63:32] = si20 << 12` (extends lu12i.w to 52 bits)
`lu52i.d`       | 12          | `rd[63:52] = si12` (extends to full 64 bits)

The `pcalau12i` instruction is the LoongArch equivalent of RISC-V's `auipc`. It provides the upper 20 bits (shifted by 12) of a PC-relative offset. The lower 12 bits are then provided by instructions with 12-bit signed immediates such as `addi.d`, `ld.d`, `st.d`, or `jirl`.

Because these lower 12-bit offsets are signed, the value passed to `pcalau12i` must be adjusted by `0x800` to ensure the combined address works correctly. Dynasm-rs performs this adjustment automatically when labels or full offsets are used as the `pcalau12i` argument.

This results in the following behaviour for `pcalau12i`:

- `pcalau12i r4, 0x12345000`: `r4 = pc + 0x12345000`
- `pcalau12i r4, 0x123457FF`: `r4 = pc + 0x12345000`
- `pcalau12i r4, 0x12345800`: `r4 = pc + 0x12346000`
- `pcalau12i r4, 0x12346000`: `r4 = pc + 0x12346000`

#### Lower immediate instructions

After use of `pcalau12i rd, offset32` to load the upper portion of a PC-relative offset, the following instructions can be used to fill in the lowest 12 bits:

Table 7: Lower immediate instruction formats for PC-relative operations

Instruction formats                                                  | Function
:---------------------------------------------------------------------|:------------------
`addi.d rd, rd, offset32 & 0xFFF`                                     | load `pc + offset32` into `rd`
`jirl ra, rd, offset32 & 0xFFF`                                       | Jump (with link) to `pc + offset32`
`ld.d rd, [rd, offset32 & 0xFFF]`<br>and `ld.w`/`ld.bu`/`ld.hu`/`ld.wu`| loads a value from `[pc + offset32]` into `rd`
`st.d rd, [rd, offset32 & 0xFFF]`<br>and `st.w`/`st.b`/`st.h`        | stores `rd` to `[pc + offset32]`
`fld.d fd, [rd, offset32 & 0xFFF]`<br>and `fld.s`                    | loads a FP value from `[pc + offset32]` into `fd`
`fst.d fd, [rd, offset32 & 0xFFF]`<br>and `fst.s`                    | stores FP value `fd` to `[pc + offset32]`

These instructions can also be used with dynamic offsets, in which case dynasm-rs takes care of the masking automatically. Note that the program counter referenced is the address of the `pcalau12i` instruction. When dynasm-rs labels are used as the offset, the offset will evaluate to different values in the `pcalau12i` instruction and the subsequent instruction. To remedy this, an offset equal to the spacing between these instructions must be added to the relocation in the subsequent instruction:

```rust
->target_label:
.u32 0xAABBCCDD
<some code>
pcalau12i r4, ->target_label
ld.d r4, [r4, ->target_label + 4] // loads 0xAABBCCDD
```

### Addressing modes

LoongArch64 supports three addressing modes for memory access:

1. **Register-offset**: `[base, offset]` — base register + signed 12-bit immediate offset
   - Used by: `ld.d`, `ld.w`, `ld.b`, `ld.h`, `ld.bu`, `ld.hu`, `ld.wu`, `st.d`, `st.w`, `st.b`, `st.h`, `fld.d`, `fld.s`, `fst.d`, `fst.s`
   - Example: `ld.d r4, [r3, 16]`

2. **Register-indexed**: base + index register (no bracket syntax)
   - Used by: `ldx.d`, `ldx.w`, `ldx.wu`, `stx.d`, `stx.w`, `fldx.d`, `fldx.s`, `fstx.d`, `fstx.s`
   - Example: `ldx.d r4, r12, r5` (address = r12 + r5)

3. **PC-relative**: via `pcalau12i` + lower immediate (see PC-relative instructions section above)

### Pseudo-Instructions

The LoongArch ISA defines several pseudo-instructions that expand to sequences of real instructions. The following table lists multi-instruction pseudo-instructions supported by dynasm-rs:

Table 8: LoongArch64 pseudo-instructions

Instruction          | Equivalent dynasm-rs instructions                          | Function
:--------------------|:-----------------------------------------------------------|:----------------------------------
`la rd, label`       | `pcalau12i rd, label` <br>`addi.d rd, rd, label + 4`       | PC-relative load address
`ld.d rd, label, rt` | `pcalau12i rt, label` <br>`ld.d rd, [rt, label + 4]`       | PC-relative load 64-bit
`ld.w rd, label, rt` | `pcalau12i rt, label` <br>`ld.w rd, [rt, label + 4]`       | PC-relative load 32-bit signed
`ld.wu rd, label, rt`| `pcalau12i rt, label` <br>`ld.wu rd, [rt, label + 4]`      | PC-relative load 32-bit unsigned
`st.d rd, label, rt` | `pcalau12i rt, label` <br>`st.d rd, [rt, label + 4]`       | PC-relative store 64-bit
`st.w rd, label, rt` | `pcalau12i rt, label` <br>`st.w rd, [rt, label + 4]`       | PC-relative store 32-bit
`call label`         | `pcalau12i r1, label` <br>`jirl r1, r1, label + 4`         | 32-bit relative call
`call rd, label`     | `pcalau12i rd, label` <br>`jirl rd, rd, label + 4`         | 32-bit relative call, return address to `rd`
`tail label`         | `pcalau12i r12, label` <br>`jirl r0, r12, label + 4`       | 32-bit relative tail call

Note: `rt` in these instructions is a temporary register used during address generation. Its value is not preserved.

### Load immediate

LoongArch64 does not have a single `li` pseudo-instruction that loads arbitrary 64-bit immediates like RISC-V. Instead, it uses a sequence of up to 4 instructions to construct a 64-bit constant:

Table 9: LoongArch64 constant generation sequence

Instruction         | Bits loaded     | Value range
:-------------------|:----------------|:----------------------
`lu12i.w rd, imm20` | [31:12]         | fills bits 12-31 with `si20 << 12`
`lu32i.d rd, imm20` | [51:32]         | fills bits 32-51 with `si20 << 12` (sign-extends to bit 63)
`lu52i.d rd, imm12` | [63:52]         | fills bits 52-63 with `si12`
`addi.d rd, rd, imm12`| [11:0]        | fills bits 0-11 with `si12`

A typical full 64-bit constant generation sequence:

```
lu12i.w  rd, upper20     // rd[31:12] = si20 << 12, rd[63:32] = sign extension
addi.d   rd, rd, lower12 // rd[11:0] = si12, corrects the lower bits
lu32i.d  rd, mid20       // rd[51:32] = si20 << 12 (if needed for 52-bit values)
lu52i.d  rd, top12       // rd[63:52] = si12 (if needed for 64-bit values)
```

### Upper immediate instructions

Similar to RISC-V, the behavior of LoongArch's upper immediate instructions in dynasm-rs differs from the GNU assembler. Where the GNU assembler expects the argument to be the field value (i.e., already shifted right), dynasm-rs expects the argument to be the expected result value of the instruction. This is done out of consistency — every other immediate in the instruction set is encoded this way in dynasm-rs.

Table 10: Upper immediate syntax

GNU style                    | Dynasm-rs style             | Result
:----------------------------|:----------------------------|:----------------------
`lu12i.w rd, 0x12345`        | `lu12i.w rd, 0x12345000`    | `rd == 0x12345000`
`pcalau12i rd, 0x12345`      | `pcalau12i rd, 0x12345000`  | `rd == pc + 0x12345000`
`pcaddu2i rd, 0x12345`       | `pcaddu2i rd, 0x12345000`   | `rd == pc + 0x12345000`

### Shifted immediates

Several LoongArch instructions have immediates that are implicitly shifted before use. In the official LoongArch Architecture Reference Manual, these are shown with already-shifted notation (e.g., `ldptr.d` uses a 14-bit offset that is shifted left by 2). In dynasm-rs, these immediates are passed as their final value, and the assembler handles the shift internally.

Table 11: Instructions with shifted immediates

Instruction           | Manual notation    | Dynasm-rs notation          | Shift
:----------------------|:-------------------|:----------------------------|:------
`ldptr.d` (ldox4.d)   | `si14 << 2`        | full offset value           | 2
`stptr.d` (stox4.d)   | `si14 << 2`        | full offset value           | 2
`ldptr.w` (ldox4.w)   | `si14 << 2`        | full offset value           | 2
`stptr.w` (stox4.w)   | `si14 << 2`        | full offset value           | 2