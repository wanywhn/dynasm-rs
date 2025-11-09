% loongarch64 assembly language reference

# Lexical structure definition

Instructions for the `loongarch64` assembling backend use the following lexical structure:

## Base units

The following base syntax units are recognized by the parser.

- `static_reg_name` matches any valid register name as seen in table 1, or any previously defined alias
- `dynamic_reg_family` matches any valid register family from table 1
- `vector_reg_name` matches `v0` up to `v31`

## Instruction

`instruction : ident ("." ident)* (arg ("," arg)* )? ;`

## Arguments

<!-- `arg : register | registerlist | labelref | reference | immediate ;` -->
`arg : register | labelref | reference | immediate ;`

`register : scalar_reg | vector_reg ;`

`scalar_reg : static_reg_name | dynamic_reg_family "(" expr ")"`

`vector_reg : ( vector_reg_name | "V" "(" expr ")" ) "." vector_width_spec element_specifier ? ;`

<!-- `register_list : "{ comma_list | dash_list | amount_list "}" element_specifier ? ;` -->

`comma_list : register ("," register) * ;`

`dash_list : register "-" register ;`

`amount_list : register "*" expr ;`

`element_specifier : "[" expr "]" ;`

`reference : "[" refitem ("," refitem)* "]" "!"? ;`

`refitem : register | immediate ;`

`immediate : "#"? expr ;`

# Reference

## Instructions

The assembly language used by dynasm-rs in loongarch64 mode is inspired by the assembly dialect used by the GNU assembler. Several additions have been made to support dynamic registers, and to ensure the Rust parser can parse it.

A significant difference exists in the syntax used for memory references. The GNU assembler uses `offset(base_register)` syntax for these. Use of this syntax in dynasm-rs would cause parsing ambiguities as it is unclear if the given expression should be parsed as an immediate that contains a function call, or a memory reference. Therefore, the dynasm-rs RISC-V assembly language uses arm-style `[base, offset]` memory references.

### Operands

#### Register

There are two ways to reference registers in dynasm-rs, either via their static name, or via dynamic register references. Dynamic register references allow the exact register choice to be made at runtime. Note that this does prevent optimizations to register-specific forms. However, the expression inside a dynamic register reference may be evaluated multiple times.

The following table lists all available static registers, their dynamic family name and their encoding when they are used dynamically.

Table 1: dynasm-rs registers (loongarch64)

Family            | 64-bit      | 64-bit       | vector   |
-----------------:|:------------|:-------------|:---------|
Dynamic Encoding  | `R`         | `F`          | `V`      |
              `0` | `r0/zero`   | `f0/fa0`     | `v0`     |
              `1` | `r1/ra`     | `f1/fa1`     | `v1`     |
              `2` | `r2/tp`     | `f2/fa2`     | `v2`     |
              `3` | `r3/sp`     | `f3/fa3`     | `v3`     |
              `4` | `r4/a0`     | `f4/fa4`     | `v4`     |
              `5` | `r5/a1`     | `f5/fa5`     | `v5`     |
              `6` | `r6/a2`     | `f6/fa6`     | `v6`     |
              `7` | `r7/a3`     | `f7/fa7`     | `v7`     |
              `8` | `r8/a4`     | `f8/ft0`     | `v8`     |
              `9` | `r9/a5`     | `f9/ft1`     | `v9`     |
             `10` | `r10/a6`    | `f10/ft2`    | `v10`    |
             `11` | `r11/a7`    | `f11/ft3`    | `v11`    |
             `12` | `r12/t0`    | `f12/ft4`    | `v12`    |
             `13` | `r13/t1`    | `f13/ft5`    | `v13`    |
             `14` | `r14/t2`    | `f14/ft6`    | `v14`    |
             `15` | `r15/t3`    | `f15/ft7`    | `v15`    |
             `16` | `r16/t4`    | `f16/ft8`    | `v16`    |
             `17` | `r17/t5`    | `f17/ft9`    | `v17`    |
             `18` | `r18/t6`    | `f18/ft10`   | `v18`    |
             `19` | `r19/t7`    | `f19/ft11`   | `v19`    |
             `20` | `r20/t8`    | `f20/ft12`   | `v20`    |
             `21` | `r21/u0`    | `f21/ft13`   | `v21`    |
             `22` | `r22/fp`    | `f22/ft14`   | `v22`    |
             `23` | `r23/s0`    | `f23/ft15`   | `v23`    |
             `24` | `r24/s1`    | `f24/fs0`    | `v24`    |
             `25` | `r25/s2`    | `f25/fs1`    | `v25`    |
             `26` | `r26/s3`    | `f26/fs2`    | `v26`    |
             `27` | `r27/s4`    | `f27/fs3`    | `v27`    |
             `28` | `r28/s5`    | `f28/fs4`    | `v28`    |
             `29` | `r29/s6`    | `f29/fs5`    | `v29`    |
             `30` | `r30/s7`    | `f30/fs6`    | `v30`    |
             `31` | `r31/s8`    | `f31/fs7`    | `v31`    |

When used statically, the notation simply matchers the given name in the table. When used dynamically, the syntax is similar to a function call: `X(reg_number)`, where reg_number is one of the given dynamic encodings listed in the table.
Note the `reg_number` can be of an arbitrary type that implements `Into<u8>`.

<!-- #### Register lists

Several vector instructions in aarch64 address a list of registers as single operands. There are several syntaxes supported by dynasm-rs for register lists:

Table 2: dynasm-rs register list types

Type          | Example
-------------:|:---------
Comma list    | `{ Vn.B, Vn+1.B, Vn+2.B, Vn+3.B }`
Dash list     | `{ Vn.B - Vn+3.B }`
Amount list   | `{ Vn.B * 4 }`

Each of these list notations is interpreted exactly the same by dynasm-rs. The first two are also standard ARM notation, the third format is added by dynasm-rs to handle dynamic registers in register lists as otherwise the amount could only be calculated at runtime. Just like vector registers, register lists support an optional element specifier after them: `{ Vn.B * 4 }[1]`. -->

#### Jump targets

All flow control instructions and instructions featuring PC-relative addressing have a jump target as argument. This jump target will feature a label reference as described in the common language reference. Note that this reference must be encoded in a limited amount of bits due to the fixed-width loongarch64 instruction set, so check the instruction reference to see what the maximum offset range is.

#### Memory references

As a load-store architecture, the loongarch64 instruction set only has a limited amount of instructions capable of addressing memory. Further more, it supports a limited set of addressing modes. The available addressing modes for each instruction are listed directly in the instruction reference. All possible addressing modes are summarized in the table below as well.

Table 3: dynasm-rs memory reference formats

Syntax   | Explanation
:--------|:-----------
<code>[Xn&#124;SP]</code> | A `XSP` family register is used as the address to be resolved.
<code>[Xn&#124;SP {, #imm } ]</code> | A `XSP` family register is used as base with an optional integer offset as the address to be resolved.
<code>[Xn&#124;SP {, labelref} ]</code> | The lower 12 bits of a relocation are added to an addres in the `XSP` family register. See the section on pc-relative instructions for further details.

#### Immediates
