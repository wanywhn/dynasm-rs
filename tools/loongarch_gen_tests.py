
#!/usr/bin/python3

import argparse
import random
import re
from pathlib import Path

# LoongArch instruction blacklist
LOONGARCH_BLACKLIST = {
    # 已知不支持的指令
    'xxx.unknown',
}

def main():
    parser = argparse.ArgumentParser(
        description="Generate LoongArch test cases from opmap definitions")
    parser.add_argument("input_file", type=Path, help="Input file with instruction definitions") 
    parser.add_argument("output_file", type=Path, help="Output file for generated test cases")
    args = parser.parse_args()

    # Read input file
    with args.input_file.open("r", encoding="utf-8") as f:
        lines = [line.strip() for line in f if line.strip()]

    test_cases = []
    for line in lines:
        parts = line.split('\t')
        if len(parts) < 4:
            continue

        mnemonic_args = parts[0]
        constraints = parts[1]
        isa = parts[2]
        extensions = parts[3] if len(parts) > 3 else ""

        # Skip blacklisted instructions
        mnemonic = mnemonic_args.split(' ', 1)[0].strip('"')
        if any(blacklisted in mnemonic for blacklisted in LOONGARCH_BLACKLIST):
            print(f"Skipping {mnemonic} (matches blacklist pattern)")
            continue
            
        # Debug output
        # if args.verbose:
            # print(f"Processing: {mnemonic_args}")

        # Generate test case
        test_case = generate_test_case(mnemonic_args, constraints, isa, extensions)
        test_cases.append(test_case)

    # Write output file
    with args.output_file.open("w", encoding="utf-8") as f:
        for case in test_cases:
            f.write(f"{case}\n")

def generate_test_case(mnemonic_args, constraints, isa, extensions):
    # Split mnemonic and arguments
    parts = mnemonic_args.split(' ', 1)
    mnemonic = parts[0].strip('"')
    args = parts[1] if len(parts) > 1 else ""

    # Generate dynasm format
    dynasm_format = f"{mnemonic} {clean_args(args)}"

    # Generate GNU as format 
    gnu_as_format = f"{mnemonic} {convert_args_to_gnu_as(args)}".strip('"')

    # Return only 2 fields: dynasm format and GNU as format
    return f"{dynasm_format}\t{gnu_as_format}"

def clean_args(args):
    """Remove parameter markers from arguments"""
    return args.replace('"', '').replace('<', '').replace('>', '')

def convert_args_to_gnu_as(args):
    """Convert dynasm args format to GNU as format"""
    # First remove all parameter markers
    clean_args = args.replace('<', '').replace('>', '')
    parts = clean_args.split(', ')
    converted = []
    
    for part in parts:
        # Handle register arguments
        if part.startswith("R,"):
            reg_num = part[2:]
            converted.append(f"$r{reg_num}")
        elif part.startswith("F,"):
            reg_num = part[2:]
            converted.append(f"$f{reg_num}")
        elif part.startswith("X,"):
            reg_num = part[2:]
            converted.append(f"$x{reg_num}")
        elif part.startswith("V,"):
            reg_num = part[2:]
            converted.append(f"$vr{reg_num}")
        elif part.startswith("XV,"):
            reg_num = part[3:]
            converted.append(f"$xvr{reg_num}")
        # Handle immediate arguments    
        elif part.startswith("Imm,"):
            imm_num = part[4:]
            converted.append(f"{imm_num}")
        # Handle offset arguments
        elif part.startswith("Off,"):
            off_num = part[4:]
            converted.append(f".+{off_num}")
        # Handle memory references
        elif part.startswith("[R,"):
            if ", Imm," in part:
                # Memory with offset
                inner = part[1:].split(', ')
                reg_num = inner[0][2:]
                imm_num = inner[1][4:]
                converted.append(f"[r{reg_num}, #{imm_num}]")
            else:
                # Simple memory
                reg_num = part[2:]
                converted.append(f"[r{reg_num}]")
        else:
            converted.append(part)
    
    return ', '.join(converted)

if __name__ == '__main__':
    main()
