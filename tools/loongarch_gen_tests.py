
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
    parser.add_argument("--attempts", type=int, default=1, 
                       help="Number of test cases to generate per instruction")
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
        constraints = eval(parts[1], {
        'Range': Range,
        'R': R,
        'F': F,
        'V': V,
        'X': X,
        'C': C,
        'T': T,
        'List': List,
        'Special': Special
    })
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

        # Generate multiple test cases per instruction
        for _ in range(args.attempts):
            test_case = generate_test_case(mnemonic_args, constraints, isa, extensions)
            test_cases.append(test_case)

    # Write output file
    with args.output_file.open("w", encoding="utf-8") as f:
        for case in test_cases:
            f.write(f"{case}\n")

def generate_test_case(mnemonic_args, constraints, isa, extensions):
    # Split mnemonic and arguments
    parts = mnemonic_args.strip('"').split(' ', 1)
    mnemonic = parts[0]
    args = parts[1] if len(parts) > 1 else ""

    # Parse arguments and apply constraints
    parsed_args = []
    arg_parts = args.split(', ') if args else []
    for i, part in enumerate(arg_parts):
        if part.startswith(('R,', 'F,', 'X,', 'V,', 'XV,')):
            reg_type = part[0]
            reg_num = constraints[i].create_value() if i < len(constraints) else random.randint(0, 31)
            parsed_args.append(f"{reg_type},{reg_num}")
        elif part.startswith('Imm,'):
            imm_val = constraints[i].create_value() if i < len(constraints) else random.randint(0, 255)
            parsed_args.append(f"Imm,{imm_val}")
        elif part.startswith('Off,'):
            off_val = constraints[i].create_value() if i < len(constraints) else random.randint(0, 1024)
            parsed_args.append(f"Off,{off_val}")
        else:
            parsed_args.append(part)

    # Generate formats
    dynasm_format = f"{mnemonic} {', '.join(parsed_args)}"
    gnu_as_format = f"{mnemonic} {convert_args_to_gnu_as(', '.join(parsed_args))}"

    return f"{dynasm_format}\t{gnu_as_format}"

# Constraint base class
class Constraint:
    def __init__(self):
        pass
        
    def create_value(self, history=None):
        """Generate a value based on constraint rules"""
        raise NotImplementedError()

# LoongArch specific constraints
class Range(Constraint):
    """Range constraint for immediate values"""
    def __init__(self, min, max, step):
        self.min = min
        self.max = max
        self.step = step
        
    def create_value(self, history=None):
        return random.randrange(self.min, self.max, self.step)

class R(Constraint):
    """Register constraint"""
    def __init__(self, mask):
        self.mask = mask
        self.valid_regs = [i for i in range(32) if (mask & (1 << i))]
        
    def create_value(self, history=None):
        return random.choice(self.valid_regs)

class F(Constraint):
    """Floating point register constraint"""
    def __init__(self, mask):
        self.mask = mask
        self.valid_regs = [i for i in range(32) if (mask & (1 << i))]
        
    def create_value(self, history=None):
        return random.choice(self.valid_regs)

class List(Constraint):
    """List constraint for fixed options"""
    def __init__(self, *options):
        self.options = options
        
    def create_value(self, history=None):
        return random.choice(self.options)

class C(Constraint):
    """Condition constraint"""
    def __init__(self, cond):
        self.cond = cond
        
    def create_value(self, history=None):
        return self.cond

class T(Constraint):
    """Template register constraint"""
    def __init__(self, mask):
        self.mask = mask
        self.valid_regs = [i for i in range(32) if (mask & (1 << i))]
        
    def create_value(self, history=None):
        return random.choice(self.valid_regs)

class X(Constraint):
    """Extended register constraint"""
    def __init__(self, mask):
        self.mask = mask
        self.valid_regs = [i for i in range(32) if (mask & (1 << i))]
        
    def create_value(self, history=None):
        return random.choice(self.valid_regs)

class V(Constraint):
    """Vector register constraint"""
    def __init__(self, mask):
        self.mask = mask
        self.valid_regs = [i for i in range(32) if (mask & (1 << i))]
        
    def create_value(self, history=None):
        return random.choice(self.valid_regs)

class T(Constraint):
    """Template type constraint"""
    def __init__(self, type):
        self.type = type
        
    def create_value(self, history=None):
        # Return the template type as-is
        return self.type

class Special(Constraint):
    """Special constraint for complex cases"""
    def __init__(self, type):
        self.type = type
        
    def create_value(self, history=None):
        if self.type == "loongarch_special":
            return random.choice([0, 1, 2, 3])
        return 0

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
