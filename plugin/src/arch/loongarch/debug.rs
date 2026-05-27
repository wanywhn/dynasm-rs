use super::loongarchdata::{Command, Matcher, Relocation, Opdata, ISAFlags, ExtensionFlags};
use super::LoongArchTarget;

use std::fmt::Write;

#[cfg(feature = "dynasm_opmap")]
pub fn create_opmap() -> String {
    let mut s = String::new();

    let mut mnemonics: Vec<_> = super::loongarchdata::mnemonics().cloned().collect();
    mnemonics.sort();
    for mnemonic in mnemonics {
        // get the data for this mnemonic
        let data = super::loongarchdata::get_mnemonic_data(mnemonic).unwrap();
        let formats = data.into_iter()
            .map(|x| format!("{} {}", format_opdata(mnemonic, x), format_features(x)))
            .map(|x| x.replace(">>> ", ""))
            .collect::<Vec<_>>();

        // push mnemonic name as title
        write!(s, "### {}\n```insref\n{}\n```\n", mnemonic, formats.join("\n")).unwrap();
    }
    s
}

#[cfg(feature = "dynasm_extract")]
pub fn extract_opmap() -> String {
    let mut buf = Vec::new();

    let mut mnemonics: Vec<_> = super::loongarchdata::mnemonics().cloned().collect();
    mnemonics.sort();

    for mnemonic in mnemonics {
        // get the data for this mnemonic
        let data = super::loongarchdata::get_mnemonic_data(mnemonic).unwrap();

        buf.extend(
            data.into_iter()
            .map(|x| extract_opdata(mnemonic, x))
        );
    }

    buf.join("\n")
}

pub fn format_opdata_list(name: &str, data: &[Opdata], _target: LoongArchTarget) -> String {
    let mut forms = Vec::new();

    for data in data {

        forms.push(format!("{} {}", format_opdata(name, data), format_features(data)));
    }

    forms.join("\n")
}


pub fn format_opdata(name: &str, data: &Opdata) -> String {
    let mut buf = format!(">>> {}", name);

    let mut first = true;
    for matcher in data.matchers {
        if first {
            buf.push(' ');
            first = false;
        } else {
            buf.push_str(", ");
        }

        match matcher {
            Matcher::R => buf.push_str("rd"),
            Matcher::F => buf.push_str("fd"),
            Matcher::Reg(regid) => write!(buf, "{}", regid).unwrap(),
            Matcher::Ref => buf.push_str("[base]"),
            Matcher::RefOffset => buf.push_str("[base, offset]"),
            Matcher::Imm => buf.push_str("imm"),
            Matcher::Offset => buf.push_str("target"),
            Matcher::Ident => buf.push_str("ident"),
            Matcher::C => buf.push_str("fcc"),
            Matcher::T => buf.push_str("scratch"),
            Matcher::V => buf.push_str("lsx-reg"),
            Matcher::X => buf.push_str("lasx-reg"),
            Matcher::FCSR => buf.push_str("fcsr"),
            Matcher::RefLabel => write!(buf, "RefLabel").unwrap(),
        }
    }

    // Add constraints
    let constraints = format_constraints(data);
    if !constraints.is_empty() {
        write!(buf, " ({})", constraints).unwrap();
    }

    buf
}

pub fn sum_adjacent_diffs(array: &&[u8]) -> u16 {
    assert!(array.len() % 2 == 0, "Array length must be even");
    
    let mut sum = 0;
    for chunk in array.chunks_exact(2) {
        sum += (chunk[0] as i16 - chunk[1] as i16).abs() as u16;
    }
    sum
}

fn format_constraints(data: &Opdata) -> String {
    let mut constraints = Vec::new();

    for command in data.commands {
        match command {
            Command::R(_) => (),
            Command::Rno0(_) => constraints.push("rd cannot be r0".to_string()),
            Command::Rdiff(_) => constraints.push("rd cannot eque previous ".to_string()),
            Command::UImm(_start, _end) => {
                // TODO: implement constraint formatting
                // let s = format!("0 <= imm <= {}", (1u32 << (end.wrapping_sub(*start) as u8)) - 1);
                // constraints.push(s);
            },
            Command::SImm(_start, _end) => {
                // TODO: implement constraint formatting
                // let s = format!("{} <= imm <= {}", - 1i32 << (end.wrapping_sub(*start) as u8) /2  - 1, 1i32 << (end.wrapping_sub(*start)as u8) /2);
                // constraints.push(s);
            },
            Command::Ufields(array) => {
                                let sum = sum_adjacent_diffs(array);
                                let s = format!("0 <= imm <= {}", sum);
                                constraints.push(s);
                            },
            Command::Usum(_, bits) => {
                // TODO add pre value?
                let s = format!("1 <= value <= {} - prev arg", 1u32 << bits);
                constraints.push(s);
            },
            Command::Ulep(_, _) => {
                // TODO add pre value?
                let s = format!("1 <= value <= prev arg");
                constraints.push(s);
            },
            Command::Sfields(array) => {
                                let sum = sum_adjacent_diffs(array) as i16;
                                let s = format!("{} <= imm <= {}", - sum / 2  - 1, sum / 2);
                                constraints.push(s);
                            },
            Command::Offset(reloc) => match reloc {
                                Relocation::B16 => constraints.push("16-bit offset, 2-byte aligned".to_string()),
                                Relocation::B26 => constraints.push("26-bit offset, 2-byte aligned".to_string()),
                                Relocation::PCALA_LO12 => constraints.push("12-bit PC-relative offset".to_string()),
                                Relocation::PCALA_HI20 => constraints.push("20-bit PC-relative high offset".to_string()),
                                _ => (),
                            },
            Command::Next | Command::Repeat => (),
            Command::F(_) => (),
            Command::C(_) => (),
            Command::FCSR(_) => {
                constraints.push("fcsr: 0 - 3".to_string());
            },
            Command::T(_) => (),
            Command::V(_) => (),
            Command::X(_) => (),
            Command::Uscaled(_, bits, scale) => {
                        let s = format!("value <= {}, value = {} * N", (1u32 << (bits + scale)) - 1, 1u32 << scale);
                        constraints.push(s);

                    }
            Command::Sscaled(_, bits, scale) => {
                        let s = format!("-{} <= value <= {}, value = {} * N", 1u32 << (bits + scale - 1), (1u32 << (bits + scale - 1)) - 1, 1u32 << scale);
                        constraints.push(s);
                    }
            Command::Usubone(_, bitlen) => {
                let s = format!("1 <= value <= {}", 1u32 << bitlen);
                constraints.push(s);
            },
        }
    }

    constraints.join(", ")
}

pub fn format_features(data: &Opdata) -> String {
    let start = if data.isa_flags.contains(ISAFlags::LA32) && !data.isa_flags.contains(ISAFlags::LA64) {
        "LA32"
    } else if data.isa_flags.contains(ISAFlags::LA64) && !data.isa_flags.contains(ISAFlags::LA32) {
        "LA64"
    } else {
        "LA32/64"
    };

    let mut items = Vec::new();

    for ext_flags in data.ext_flags.iter() {
        let mut item = start.to_string();
        item.push('_');
        
        if ext_flags.contains(ExtensionFlags::Ex_BASE) {
            item.push_str("BASE");
        }
        if ext_flags.contains(ExtensionFlags::Ex_BIT) {
            item.push_str("_BIT");
        }
        if ext_flags.contains(ExtensionFlags::Ex_F) {
            item.push_str("_F");
        }
        if ext_flags.contains(ExtensionFlags::Ex_D) {
            item.push_str("_D");
        }
        if ext_flags.contains(ExtensionFlags::Ex_LSX) {
            item.push_str("_LSX");
        }
        if ext_flags.contains(ExtensionFlags::Ex_LASX) {
            item.push_str("_LASX");
        }
        if ext_flags.contains(ExtensionFlags::Ex_LVZ) {
            item.push_str("_LVZ");
        }
        if ext_flags.contains(ExtensionFlags::Ex_LBT) {
            item.push_str("_LBT");
        }
        if ext_flags.contains(ExtensionFlags::Ex_PRIV) {
            item.push_str("_PRIV");
        }

        items.push(item);
    }

    format!("({})", items.join(" or "))
}

#[cfg(feature = "dynasm_extract")]
pub fn extract_opdata(name: &str, data: &Opdata) -> String {
    let mut buf = format!("\"{}", name);

    let mut first = true;
    let mut arg_idx = 0;

    let constraints = extract_constraints(data).join(", ");
    for matcher in data.matchers {
        if first {
            buf.push(' ');
            first = false;
        } else {
            buf.push_str(", ");
        }

            match matcher {
                Matcher::R => write!(buf, "<R,{}>", arg_idx).unwrap(),
                Matcher::F => write!(buf, "<F,{}>", arg_idx).unwrap(),
                Matcher::Reg(regid) => write!(buf, "{}", regid).unwrap(),
                Matcher::Ref => write!(buf, "[<R,{}>]", arg_idx).unwrap(),
                Matcher::RefOffset => write!(buf, "[<R,{}>, <Imm,{}>]", arg_idx, arg_idx + 1).unwrap(),
                Matcher::RefLabel => write!(buf, "[<R,{}>, <Off,{}>]", arg_idx, arg_idx + 1).unwrap(),
                Matcher::Imm => write!(buf, "<Imm,{}>", arg_idx).unwrap(),
                Matcher::Offset => write!(buf, "<Off,{}>", arg_idx).unwrap(),
                Matcher::Ident => write!(buf, "<Ident,{}>", arg_idx).unwrap(),
                Matcher::C => write!(buf, "<C,{}>", arg_idx).unwrap(),
                Matcher::T => write!(buf, "<T,{}>", arg_idx).unwrap(),
                Matcher::V => write!(buf, "<V,{}>", arg_idx).unwrap(),
                Matcher::X => write!(buf, "<X,{}>", arg_idx).unwrap(),
                Matcher::FCSR => write!(buf, "<FCSR,{}>", arg_idx).unwrap(),
            }

        arg_idx += match matcher {
            Matcher::RefOffset 
            | Matcher::RefLabel => 2,
            Matcher::Reg(_) => 0,
            _ => 1
        };
    }

    write!(buf, "\"\t{{{}}}\t", constraints).unwrap();

    buf.push_str(&extract_arch_flags(data));

    buf
}

#[cfg(feature = "dynasm_extract")]
fn extract_constraints(data: &Opdata) -> Vec<String> {
    let mut constraints = Vec::new();
    let mut arg_idx = 0;

    for command in data.commands {
        let constraint = match command {
            Command::R(_) => format!("R(0xFFFFFFFF)"),
            Command::Rdiff(_) => format!("Rdiff(0xFFFFFFFF)"),
            Command::Rno0(_) => format!("R(0xFFFFFFFE)"),
            Command::UImm(start, len) => format!("Range(0, {}, {})", 1u32 << len, 1),
            Command::SImm(start, len) => format!("Range(-{}, {}, {})", 
                                1u32 << (len - 1), (1u32 << (len - 1)) - 1, 1u32),
            Command::Offset(Relocation::B16) => format!("Range(-{}, {}, {})", 1<<15, 1<<15, 4),
            Command::Offset(Relocation::B21) => format!("Range(-{}, {}, {})", 1<<20, 1<<20, 4),
            Command::Offset(Relocation::B26) => format!("Range(-{}, {}, {})", 1<<25, 1<<25, 4),
            Command::Offset(Relocation::ABS_HI20) => format!("Range(-{}, {}, {})", 1u64<<19, 1u64<<19, 1),
            Command::Offset(Relocation::PCADD_SHIFT2) => format!("Range(-{}, {}, {})", 1u64<<21, 1u64<<21, 4),
            Command::Offset(Relocation::PCADD_SHIFT12) => format!("Range(-{}, {}, {})", 1u64<<31, 1u64<<31, 4096),
            Command::Offset(Relocation::PCADD_SHIFT18) => format!("Range(-{}, {}, {})", 1u64<<37, 1u64<<37, 262144),
            Command::Offset(Relocation::SI14) => format!("Range(-{}, {}, {})", 1u64<<13, 1u64<<13, 4),
            Command::Offset(Relocation::SI12) => format!("Range(-{}, {}, {})", 1u64<<11, 1u64<<11, 1),
            Command::Offset(Relocation::PCALA_LO12) => format!("Range(-{}, {}, {})", 1u64<<11, 1u64<<11, 1),
            Command::Offset(Relocation::PCALA_HI20) => format!("Range(-{}, {}, {})", 1u64<<31, 1u64<<31, 4096),
            // TODO: is this need?
            Command::Offset(Relocation::LITERAL32) => format!("Range(-{}, {}, {})", 1<<31, 1<<31, 1),
            Command::Offset(Relocation::LITERAL64) => format!("Range(-{}, {}, {})", 1u64<<63, 1u64<<63, 1),
            Command::Offset(Relocation::LITERAL8) => format!("Range(-{}, {}, {})", 1<<31, 1<<31, 1),
            Command::Offset(Relocation::LITERAL16) => format!("Range(-{}, {}, {})", 1u64<<63, 1u64<<63, 1),
            Command::Next | Command::Repeat => continue,
            Command::F(_) => format!("F(0xFFFFFFFF)"),
            Command::C(_) => format!("C(0x7)"),
            Command::FCSR(_) => format!("FCSR(0x3)"),
            Command::T(_) => format!("T(0xFFFFFFFF)"),
            Command::V(_) => format!("V(0xFFFFFFFF)"),
            Command::X(_) => format!("X(0xFFFFFFFF)"),
            Command::Ufields(items) => {
                        format!("Range(0, {}, {})", 1u32 << items.iter()
                        .enumerate()
                        .filter(|(i, _)| i % 2 != 0)
                        .map(|(_, &x)| x as u32).sum::<u32>(), 1)
                    },
            Command::Sfields(items) => {
                        let l = items.iter()
                        .enumerate()
                        .filter(|(i, _)| i % 2 != 0)
                        .map(|(_, &x)| x as u32).sum::<u32>() - 1;
                        format!("Range(-{}, {}, {})", 1u32 << l, 1u32 << l -1 , 1)
                    },
            Command::Usum(_, bits) => 
                    format!("Range2(1, {}+1, 1)", 1u32 << bits),
            Command::Ulep(_, bits) => 
                    format!("Range3(0, {}, 1)", 1u32 << bits),
            Command::Uscaled(_, len, shift) => {
                format!("Range(0, {}, {})", 1u32 << len, 1 << shift)
            }
            Command::Sscaled(_, len, shift) => {
                format!("Range(-{}, {}, {})", 1u32 << (len - 1), (1u32 << (len-1)) - 1, 1 << shift)
            },
            Command::Usubone(_, bitlen) => {
                format!("Range(1, {}, {})", (1u32 << bitlen) + 1, 1)
            }
        };
        constraints.push(format!("{}: {}", arg_idx, constraint));
        arg_idx += 1;
    }

    constraints
}

#[cfg(feature = "dynasm_extract")]
fn extract_arch_flags(data: &Opdata) -> String {
    let mut isa_entries = Vec::new();

    if data.isa_flags.contains(ISAFlags::LA32) {
        isa_entries.push("\"la32\"")
    }
    if data.isa_flags.contains(ISAFlags::LA64) {
        isa_entries.push("\"la64\"")
    }

    let mut ext_sets = Vec::new();

    for ext_flags in data.ext_flags.iter() {
        let mut ext_flags = ext_flags.to_string();
        ext_flags.make_ascii_lowercase();
        ext_sets.push(format!("\"{}\"", ext_flags));
    }

    format!("[{}]\t[{}]", isa_entries.join(", "), ext_sets.join(", "))
}