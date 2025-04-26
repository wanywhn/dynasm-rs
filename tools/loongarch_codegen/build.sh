#!/bin/bash
set -e

# Get the absolute path of the project root
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

# Build the code generator
echo "Building loongarch_codegen..."
cd "${PROJECT_ROOT}/tools/loongarch_codegen"
cargo build --release

# Run the generator
echo "Generating LoongArch instruction definitions..."
"${PROJECT_ROOT}/tools/loongarch_codegen/target/release/loongarch_codegen" \
    -i "${PROJECT_ROOT}/tools/loongarch_tools/loongarch_opcodes" \
    -o "${PROJECT_ROOT}/plugin/src/arch/loongarch/generated_data.rs"

echo "Done!"