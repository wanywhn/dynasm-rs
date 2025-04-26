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

pub fn format_opdata_list(name: &str, data: &[Opdata], target: LoongArchTarget) -> String {
    let mut forms = Vec::new();

    for data in data {
        if (target.is_64_bit() && !data.isa_flags.contains(ISAFlags::LA64)) || 
           (target.is_32_bit() && !data.isa_flags.contains(ISAFlags::LA32)) {
            continue
        }

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
            Command::UImm(start, end) => {
                let s = format!("0 <= imm <= {}", (1u32 << (end - start)) - 1);
                constraints.push(s);
            },
            Command::SImm(start, end) => {
                let s = format!("{} <= imm <= {}", - 1i32 << (end - start) /2  - 1, 1i32 << (end - start) /2);
                constraints.push(s);
            },
            Command::Ufields(array) => {
                let sum = sum_adjacent_diffs(array);
                let s = format!("0 <= imm <= {}", sum);
                constraints.push(s);
            },
            Command::Sfields(array) => {
                let sum = sum_adjacent_diffs(array) as i16;
                let s = format!("{} <= imm <= {}", - sum / 2  - 1, sum / 2);
                constraints.push(s);
            },
            Command::Offset(reloc) => match reloc {
                Relocation::B => constraints.push("16-bit offset, 2-byte aligned".to_string()),
                Relocation::J => constraints.push("26-bit offset, 2-byte aligned".to_string()),
                Relocation::PC32 => constraints.push("32-bit PC-relative offset".to_string()),
                _ => (),
            },
            Command::Next | Command::Repeat => (),
            Command::F(_) => todo!(),
            Command::C(_) => todo!(),
            Command::T(_) => todo!(),
            Command::V(_) => todo!(),
            Command::X(_) => todo!(),
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
            Matcher::Imm => write!(buf, "<Imm,{}>", arg_idx).unwrap(),
            Matcher::Offset => write!(buf, "<Off,{}>", arg_idx).unwrap(),
            Matcher::Ident => write!(buf, "<Ident,{}>", arg_idx).unwrap(),
            Matcher::C => todo!(),
            Matcher::T => todo!(),
            Matcher::V => todo!(),
            Matcher::X => todo!(),
        }

        arg_idx += match matcher {
            Matcher::RefOffset => 2,
            Matcher::Reg(_) => 0,
            _ => 1
        };
    }

    write!(buf, "\"\t{{{}}}\t", extract_constraints(data).join(", ")).unwrap();

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
            Command::Rno0(_) => format!("R(0xFFFFFFFE)"),
            Command::UImm(bits, scale) => format!("Range(0, {}, {})", 1u32 << bits, 1u32 << scale),
            Command::SImm(bits, scale) => format!("Range(-{}, {}, {})", 
                        1u32 << (bits - 1), 1u32 << (bits - 1), 1u32 << scale),
            Command::Offset(_) => format!("R(0xFFFFFFFF)"),
            Command::Next | Command::Repeat => continue,
            Command::F(_) => format!("R(0xFFFFFFFF)"),
            Command::C(_) => format!("R(0xFFFFFFFF)"),
            Command::T(_) => format!("R(0xFFFFFFFF)"),
            Command::V(_) => format!("R(0xFFFFFFFF)"),
            Command::X(_) => format!("R(0xFFFFFFFF)"),
            Command::Ufields(items) => format!("R(0xFFFFFFFF)"),
            Command::Sfields(items) => format!("R(0xFFFFFFFF)"),
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