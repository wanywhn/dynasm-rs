use std::str::FromStr;

#[derive(Debug, Clone, PartialEq)]
pub enum OperandType {
    // Register operands
    Rd,     // Destination register
    Rj,     // Source register 1
    Rk,     // Source register 2
    Fa,     // Floating-point destination register
    Fj,     // Floating-point source register 1
    Fk,     // Floating-point source register 2
    Va,     // Vector destination register
    Vj,     // Vector source register 1
    Vk,     // Vector source register 2

    // Immediate operands
    Uk5,    // 5-bit unsigned immediate
    Uk6,    // 6-bit unsigned immediate
    Uk12,   // 12-bit unsigned immediate
    Sk12,   // 12-bit signed immediate
    Sk14,   // 14-bit signed immediate
    Sk16,   // 16-bit signed immediate
    Sj20,   // 20-bit signed immediate for jump
    Sk20,   // 20-bit signed immediate
    Sk26,   // 26-bit signed immediate
}

#[derive(Debug, Clone)]
pub struct InstructionFormat {
    pub operands: Vec<OperandType>,
    pub encoding: u32,
}

impl InstructionFormat {
    pub fn parse(format: &str, encoding: u32) -> Option<Self> {
        let mut operands = Vec::new();

        // Parse format string
        for c in format.chars() {
            match c {
                'D' => operands.push(OperandType::Rd),
                'J' => operands.push(OperandType::Rj),
                'K' => operands.push(OperandType::Rk),
                'F' => {
                    if operands.is_empty() {
                        operands.push(OperandType::Fa);
                    } else {
                        operands.push(OperandType::Fj);
                    }
                },
                'V' => {
                    if operands.is_empty() {
                        operands.push(OperandType::Va);
                    } else {
                        operands.push(OperandType::Vj);
                    }
                },
                _ => ()
            }
        }

        // Parse immediate values
        if let Some(imm) = format.find('k') {
            let num = &format[imm+1..];
            match num {
                "5" => operands.push(OperandType::Uk5),
                "6" => operands.push(OperandType::Uk6),
                "12" => {
                    if format.contains('S') {
                        operands.push(OperandType::Sk12);
                    } else {
                        operands.push(OperandType::Uk12);
                    }
                },
                "14" => operands.push(OperandType::Sk14),
                "16" => operands.push(OperandType::Sk16),
                "20" => operands.push(OperandType::Sk20),
                "26" => operands.push(OperandType::Sk26),
                _ => return None,
            }
        }

        if format.contains("j20") {
            operands.push(OperandType::Sj20);
        }

        Some(InstructionFormat {
            operands,
            encoding,
        })
    }

    pub fn generate_matchers(&self) -> Vec<String> {
        let mut matchers = Vec::new();
        for op in &self.operands {
            let matcher = match op {
                OperandType::Rd | OperandType::Rj | OperandType::Rk => 
                    "Matcher::R".to_string(),
                OperandType::Fa | OperandType::Fj | OperandType::Fk => 
                    "Matcher::F".to_string(),
                OperandType::Va | OperandType::Vj | OperandType::Vk => 
                    "Matcher::V".to_string(),
                OperandType::Uk5 | OperandType::Uk6 | OperandType::Uk12 |
                OperandType::Sk12 | OperandType::Sk14 | OperandType::Sk16 |
                OperandType::Sk20 => 
                    "Matcher::Imm".to_string(),
                OperandType::Sj20 | OperandType::Sk26 => 
                    "Matcher::Offset".to_string(),
            };
            matchers.push(matcher);
        }
        matchers
    }

    pub fn generate_commands(&self) -> Vec<String> {
        let mut commands = Vec::new();
        for op in &self.operands {
            let command = match op {
                OperandType::Rd => "Command::R(0)".to_string(),
                OperandType::Rj => "Command::R(5)".to_string(),
                OperandType::Rk => "Command::R(10)".to_string(),
                OperandType::Fa => "Command::F(0)".to_string(),
                OperandType::Fj => "Command::F(5)".to_string(),
                OperandType::Fk => "Command::F(10)".to_string(),
                OperandType::Va => "Command::V(0)".to_string(),
                OperandType::Vj => "Command::V(5)".to_string(),
                OperandType::Vk => "Command::V(10)".to_string(),
                OperandType::Uk5 => "Command::UImm(5, 0)".to_string(),
                OperandType::Uk6 => "Command::UImm(6, 0)".to_string(),
                OperandType::Uk12 => "Command::UImm(12, 0)".to_string(),
                OperandType::Sk12 => "Command::SImm(12, 0)".to_string(),
                OperandType::Sk14 => "Command::SImm(14, 0)".to_string(),
                OperandType::Sk16 => "Command::SImm(16, 0)".to_string(),
                OperandType::Sj20 => "Command::Offset(Relocation::J)".to_string(),
                OperandType::Sk20 => "Command::SImm(20, 0)".to_string(),
                OperandType::Sk26 => "Command::Offset(Relocation::B)".to_string(),
            };
            commands.push(command);
        }
        commands
    }
}