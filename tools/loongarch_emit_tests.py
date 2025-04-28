
import os
import os.path

BLACKLIST = set()  # 初始为空，后续根据需要添加

def read_input_file(f):
    buf = []
    for line in f:
        dynasm, gas, bytes = line.split("\t")
        buf.append((dynasm.strip(), gas.strip(), bytes.strip()))
    return buf

def chunks(l, n):
    for i in range(0, len(l), n):
        yield l[i:i+n]

def emit_test_case(i, dynasm, gas, bytes):
    name = dynasm.split(' ', 1)[0].replace(".", "_")
    if name in BLACKLIST:
        return ""
    bytes = ", ".join(chunks(bytes, 2)).upper()
    error = dynasm.replace("{", "{{").replace("}", "}}")
    return """
#[test]
fn {}_{}() {{
    let mut ops = dynasmrt::loongarch::Assembler::new().unwrap();
    dynasm!(ops
        ; .arch loongarch
        ; {}
    );
    let buf = ops.finalize().unwrap();
    let hex: Vec<String> = buf.iter().map(|x| format!("{{:02X}}", *x)).collect();
    let hex = hex.join(", ");
    assert_eq!(hex, "{}", "{}");
}}
""".format(name, i, dynasm, bytes, error)

def main():
    import sys
    with open(sys.argv[1], "r", encoding="utf-8") as f:
        data = read_input_file(f)

    tests = [emit_test_case(i, dynasm, gas, bytes) for i, (dynasm, gas, bytes) in enumerate(data)]

    for i, chunk in enumerate(chunks(tests, 800)):
        with open(os.path.join(sys.argv[2], "loongarch_tests_{}.rs.gen".format(i)), "w", encoding="utf-8") as f:
            for test in chunk:
                f.write(test)

if __name__ == "__main__":
    main()
