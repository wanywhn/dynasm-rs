
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
        templates = read_opdata_file(f)

    # Generate test cases
    buf = []
    for template in templates:
        for _ in range(args.attempts):
            buf.append(template.create_entry())

    # Write output file
    with args.output_file.open("w", encoding="utf-8") as f:
        for dynasm, gas in buf:
            f.write(f"{dynasm}\t{gas}\n")

def read_opdata_file(f):
    templates = []
    context = {
        'Range': Range,
        'R': R,
        'F': F,
        'V': V,
        'X': X,
        'C': C,
        'T': T,
        'List': List,
        'Special': Special
    }

    for line in f:
        line = line.strip()
        if not line:
            continue

        parts = line.split('\t')
        if len(parts) < 4:
            continue

        mnemonic_args = parts[0]
        constraints = eval(parts[1], context)
        isa = parts[2]
        extensions = parts[3] if len(parts) > 3 else ""

        # Skip blacklisted instructions
        mnemonic = mnemonic_args.split(' ', 1)[0].strip('"')
        if any(blacklisted in mnemonic for blacklisted in LOONGARCH_BLACKLIST):
            print(f"Skipping {mnemonic} (matches blacklist pattern)")
            continue

        templates.append(OpTemplate(mnemonic_args, constraints))

    return templates

class OpTemplate:
    def __init__(self, template, constraints):
        self.template = template
        self.constraints = constraints
        self.args = parse_template(template)

    def create_entry(self):
        history = History()
        for (arg, i) in self.args:
            constraint = self.constraints[i]
            value = constraint.create_value(history)
            gas = arg.emit_gas(value)
            emitted = arg.emit_dynasm(value)
            
            history.values.append(value)
            history.emitted.append(emitted)
            history.gas.append(gas)

        dynasm_string = SUBSTITUTION_RE.sub(
            lambda m: history.emitted[int(m.group(2))], 
            self.template.strip('"'))
            
        gas_string = SUBSTITUTION_RE.sub(
            lambda m: history.gas[int(m.group(2))], 
            self.template.strip('"'))
            
        # gas_string = convert_args_to_gnu_as(gas_string)

        return dynasm_string, gas_string

class History:
    def __init__(self):
        self.values = []
        self.emitted = []
        self.gas = []

SUBSTITUTION_RE = re.compile(r"<([A-Za-z]+),([0-9]+)>")
def parse_template(template):
    matches = []
    for argty, argidx in SUBSTITUTION_RE.findall(template):
        if argty == "Imm":
            arg = Immediate()
        elif argty == "Off":
            arg = Offset()
        elif argty in "RFVX":
            arg = Register(argty)
        elif argty == "C":
            arg = Condition()
        elif argty == "T":
            arg = Template()
        else:
            raise NotImplementedError(argty)
        matches.append((arg, int(argidx)))
    return matches

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
            converted.append(f"r{reg_num}")
        elif part.startswith("F,"):
            reg_num = part[2:]
            converted.append(f"f{reg_num}")
        elif part.startswith("X,"):
            reg_num = part[2:]
            converted.append(f"x{reg_num}")
        elif part.startswith("V,"):
            reg_num = part[2:]
            converted.append(f"vr{reg_num}")
        elif part.startswith("XV,"):
            reg_num = part[3:]
            converted.append(f"xvr{reg_num}")
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

# Argument classes for LoongArch
class Register:
    def __init__(self, family):
        self.family = family
        
    def emit_gas(self, value):
        if self.family == "R":
            return f"$r{value}"
        elif self.family == "F":
            return f"$f{value}"
        elif self.family == "X":
            return f"$xr{value}"
        elif self.family == "V":
            return f"$vr{value}"
        else:
            raise NotImplementedError(self.family)
            
    def emit_dynasm(self, value):
        if self.family == "R":
            return f"r{value}"
        elif self.family == "F":
            return f"f{value}"
        elif self.family == "X":
            return f"x{value}"
        elif self.family == "V":
            return f"v{value}"
        else:
            raise NotImplementedError(self.family)

class Immediate:
    def emit_gas(self, value):
        return str(value)
        
    def emit_dynasm(self, value):
        return f"{value}"

class Offset(Immediate):
    def emit_gas(self, value):
        return str(value)
        
    def emit_dynasm(self, value):
        return f"{value}"

class Condition:
    def emit_gas(self, value):
        return str(value)
        
    def emit_dynasm(self, value):
        return f"C{value}"

class Template:
    def emit_gas(self, value):
        return str(value)
        
    def emit_dynasm(self, value):
        return f"T{value}"

if __name__ == '__main__':
    main()
