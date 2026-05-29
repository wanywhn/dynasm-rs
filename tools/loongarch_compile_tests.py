
import subprocess
import binascii

def read_test_strings(f):
    buf = []
    for line in f:
        if line:
            if not "\t" in line:
                print(line)
            dynasm, gas = line.split("\t")
            buf.append((dynasm.strip(), gas.strip()))
    return buf

def compile_with_as(asmstring):
    with open("test.s", "w", encoding="utf-8") as f:
        f.write(asmstring)
        f.write("\n")

    # 使用LoongArch工具链进行汇编
    subprocess.run(["loongarch64-unknown-linux-gnu-as", "test.s", "-o", "test.o"], check=True)
    subprocess.run(["loongarch64-unknown-linux-gnu-objcopy", "-O", "binary", "test.o", "test.bin"], check=True)

    with open("test.bin", "rb") as f:
        data = f.read()
    return data

def write_result(buf, f):
    for dynasm, gas, binary in buf:
        f.write("{}\t{}\t{}\n".format(dynasm, gas, binascii.hexlify(binary).decode("utf-8")))

def main():
    import sys
    with open(sys.argv[1], "r", encoding="utf-8") as f:
        test_strings = read_test_strings(f)

    buf = []
    # LoongArch已知不支持或需要特殊处理的指令
    unsupported_instructions = {
        # AM原子指令
        'amswap', 'amswap_db', 'amadd_db', 'amand_db',
        'amcas_db', 'ammax_db', 'ammin_db', 'amor_db',
    }
    
    for dynasm, gas in test_strings:
        # 跳过已知不支持的指令
        if any(unsupported in gas.split()[0] for unsupported in unsupported_instructions):
            print("Skipping unsupported instruction: {}".format(gas))
            continue
            
        try:
            binary = compile_with_as(gas)
            buf.append((dynasm, gas, binary))
        except subprocess.CalledProcessError as e:
            print("Compilation failed for {}: {}".format(gas, e))
        except Exception as e:
            print("Error processing {}: {}".format(gas, e))

    with open(sys.argv[2], "w", encoding="utf-8") as f:
        write_result(buf, f)

if __name__ == '__main__':
    main()
