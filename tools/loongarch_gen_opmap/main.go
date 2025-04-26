package main

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strconv"
	"strings"
)

// InsnDescription 描述一条LoongArch指令
type InsnDescription struct {
	Word       uint32            // 指令编码
	Mnemonic   string            // 助记符
	Format     *InsnFormat       // 指令格式
	OrigFormat *InsnFormat       // 原始格式(如果有)
	Attribs    map[string]string // 额外属性
}

// InsnFormat 描述指令格式
type InsnFormat struct {
	Args []*Arg // 操作数列表
}

// Arg 描述一个操作数
type Arg struct {
	Kind  ArgKind       // 操作数类型
	Slots []*Slot       // 位域位置
	Post  PostprocessOp // 后处理操作
}

// Slot 描述操作数在指令中的位域
type Slot struct {
	Offset uint // 起始位
	Width  uint // 位宽
}

// ArgKind 操作数类型枚举
type ArgKind int

const (
	ArgKindUnknown     ArgKind = 0
	ArgKindIntReg      ArgKind = 1
	ArgKindFPReg       ArgKind = 2
	ArgKindFCCReg      ArgKind = 3
	ArgKindScratchReg  ArgKind = 4
	ArgKindVReg        ArgKind = 5
	ArgKindXReg        ArgKind = 6
	ArgKindSignedImm   ArgKind = 7
	ArgKindUnsignedImm ArgKind = 8
	ArgKindComplex     ArgKind = 9 // 复合操作数
)

// PostprocessOp 描述对立即数的后处理
type PostprocessOp struct {
	Kind   PostprocessOpKind
	Amount int
}

// PostprocessOpKind 后处理类型枚举
type PostprocessOpKind int

const (
	PostprocessOpNone PostprocessOpKind = iota
	PostprocessOpAdd
	PostprocessOpShl
)

func main() {
	if len(os.Args) < 3 {
		fmt.Println("Usage: loongarch_gen_opmap <input-dir> <output-file>")
		os.Exit(1)
	}

	inputDir := os.Args[1]
	outputFile := os.Args[2]

	// 1. 读取并解析所有指令定义文件
	insns, err := parseInsnDefinitions(inputDir)
	if err != nil {
		fmt.Printf("Error parsing instruction definitions: %v\n", err)
		os.Exit(1)
	}

	// 2. 生成opmap.rs文件
	if err := generateOpmapFile(outputFile, insns); err != nil {
		fmt.Printf("Error generating opmap.rs: %v\n", err)
		os.Exit(1)
	}
}

// parseInsnDefinitions 解析指令定义目录
func parseInsnDefinitions(dir string) ([]*InsnDescription, error) {
	var insns []*InsnDescription

	// 匹配.txt文件
	files, err := filepath.Glob(filepath.Join(dir, "*.txt"))
	if err != nil {
		return nil, err
	}

	for _, file := range files {
		fileInsns, err := parseInsnFile(file)
		if err != nil {
			return nil, fmt.Errorf("%s: %v", file, err)
		}
		insns = append(insns, fileInsns...)
	}

	return insns, nil
}

// parseInsnFile 解析单个指令定义文件
func parseInsnFile(path string) ([]*InsnDescription, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}

	var insns []*InsnDescription
	lines := strings.Split(string(data), "\n")

	// 正则匹配指令行: 编码 助记符 格式 @属性
	insnRE := regexp.MustCompile(`^([0-9a-f]{8}) ([a-z][0-9a-z_.]*) +([A-Za-z0-9]+)((?: *@[0-9A-Za-z_.=]+)*)$`)
	attribRE := regexp.MustCompile(`@([0-9A-Za-z_.]+)(?:=([0-9A-Za-z_.]*))?`)

	for _, line := range lines {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}

		matches := insnRE.FindStringSubmatch(line)
		if matches == nil {
			continue
		}

		// 解析编码
		word, err := strconv.ParseUint(matches[1], 16, 32)
		if err != nil {
			return nil, fmt.Errorf("invalid instruction encoding: %s", matches[1])
		}

		// 解析属性
		attribs := make(map[string]string)
		for _, attr := range attribRE.FindAllStringSubmatch(matches[4], -1) {
			if attr[2] != "" {
				attribs[attr[1]] = attr[2]
			} else {
				attribs[attr[1]] = "true"
			}
		}

		// 解析指令格式
		format, err := parseInsnFormat(matches[3])
		if err != nil {
			return nil, err
		}

		// 解析原始格式(如果有)
		var origFormat *InsnFormat
		if origFmt, ok := attribs["orig_fmt"]; ok {
			origFormat, err = parseInsnFormat(origFmt)
			if err != nil {
				return nil, err
			}
			delete(attribs, "orig_fmt")
		}

		insn := &InsnDescription{
			Word:       uint32(word),
			Mnemonic:   matches[2],
			Format:     format,
			OrigFormat: origFormat,
			Attribs:    attribs,
		}

		insns = append(insns, insn)
	}

	return insns, nil
}

// parseInsnFormat 解析指令格式字符串
func parseInsnFormat(s string) (*InsnFormat, error) {
	if s == "EMPTY" {
		return &InsnFormat{Args: nil}, nil
	}

	var args []*Arg
	runes := []rune(s)

	for i := 0; i < len(runes); {
		arg, n, err := parseArg(runes[i:])
		if err != nil {
			return nil, err
		}
		args = append(args, arg)
		i += n
	}

	return &InsnFormat{Args: args}, nil
}

// parseArg 解析单个操作数
func parseArg(runes []rune) (*Arg, int, error) {
	if len(runes) == 0 {
		return nil, 0, errors.New("empty arg")
	}

	prefix := runes[0]
	switch prefix {
	case 'D', 'J', 'K', 'A':
		// 整数寄存器: D(rd), J(rj), K(rk), A(ra)
		return &Arg{
			Kind:  ArgKindIntReg,
			Slots: []*Slot{{Offset: regOffset(prefix), Width: 5}},
		}, 1, nil

	case 'C', 'F', 'T', 'V', 'X':
		// 其他寄存器类型
		if len(runes) < 2 {
			return nil, 0, fmt.Errorf("missing offset char after %c", prefix)
		}
		offset, err := parseOffsetCh(runes[1])
		if err != nil {
			return nil, 0, err
		}

		var kind ArgKind
		switch prefix {
		case 'C':
			kind = ArgKindFCCReg
		case 'F':
			kind = ArgKindFPReg
		case 'T':
			kind = ArgKindScratchReg
		case 'V':
			kind = ArgKindVReg
		case 'X':
			kind = ArgKindXReg
		}

		return &Arg{
			Kind:  kind,
			Slots: []*Slot{{Offset: offset, Width: regWidth(prefix)}},
		}, 2, nil

	case 'S', 'U':
		// 立即数: S(有符号), U(无符号)
		kind := ArgKindSignedImm
		if prefix == 'U' {
			kind = ArgKindUnsignedImm
		}

		slots, n, err := parseSlots(runes[1:])
		if err != nil {
			return nil, 0, err
		}

		post, nPost, err := parsePostprocessOp(runes[1+n:])
		if err != nil {
			return nil, 0, err
		}

		return &Arg{
			Kind:  kind,
			Slots: slots,
			Post:  post,
		}, 1 + n + nPost, nil
	}

	// 处理复合操作数格式(如JSd5k16, Ud5JSk12等)
	if isComplexOperand(runes) {
		return parseComplexOperand(runes)
	}

	// 跳过数字字符或后处理操作符
	if isDigit(prefix) {
		return nil, 1, nil
	}
	if prefix == 'p' {
		post, n, err := parsePostprocessOp(runes)
		if err != nil {
			return nil, 0, err
		}
		return &Arg{
			Post: post,
		}, n, nil
	}
	return nil, 0, fmt.Errorf("invalid prefix char %c", prefix)
}

// parseSlots 解析立即数的位域
func parseSlots(runes []rune) ([]*Slot, int, error) {
	var slots []*Slot
	var n int

	for n < len(runes) {
		offset, err := parseOffsetCh(runes[n])
		if err != nil {
			break
		}
		n++

		width := uint(1)
		if n < len(runes) && isDigit(runes[n]) {
			width, n = parseUint(runes[n:])
		}

		slots = append(slots, &Slot{Offset: offset, Width: width})
	}

	if len(slots) == 0 {
		return nil, 0, errors.New("no slots")
	}

	return slots, n, nil
}

// parsePostprocessOp 解析后处理操作
func parsePostprocessOp(runes []rune) (PostprocessOp, int, error) {
	if len(runes) == 0 || runes[0] != 'p' {
		return PostprocessOp{}, 0, nil
	}

	if len(runes) < 2 {
		return PostprocessOp{}, 0, errors.New("incomplete postprocess op")
	}

	kind, err := parsePostprocessOpKind(runes[1])
	if err != nil {
		return PostprocessOp{}, 0, err
	}

	amt, n := parseUint(runes[2:])
	return PostprocessOp{
		Kind:   kind,
		Amount: int(amt),
	}, 2 + n, nil
}

// parseOffsetCh 解析偏移字符
func parseOffsetCh(ch rune) (uint, error) {
	switch ch {
	case 'd':
		return 0, nil
	case 'j':
		return 5, nil
	case 'k':
		return 10, nil
	case 'a':
		return 15, nil
	case 'm':
		return 16, nil
	case 'n':
		return 18, nil
	}
	return 0, fmt.Errorf("invalid offset char %c", ch)
}

// parsePostprocessOpKind 解析后处理类型
func parsePostprocessOpKind(ch rune) (PostprocessOpKind, error) {
	switch ch {
	case 'p':
		return PostprocessOpAdd, nil
	case 's':
		return PostprocessOpShl, nil
	}
	return PostprocessOpNone, fmt.Errorf("invalid postprocess op kind %c", ch)
}

// regOffset 获取寄存器操作数的偏移量
func regOffset(ch rune) uint {
	switch ch {
	case 'D':
		return 0
	case 'J':
		return 5
	case 'K':
		return 10
	case 'A':
		return 15
	default:
		panic("invalid reg char")
	}
}

// regWidth 获取寄存器操作数的位宽
func regWidth(ch rune) uint {
	switch ch {
	case 'C':
		return 3
	case 'T':
		return 2
	default:
		return 5
	}
}

// parseUint 解析无符号整数
func parseUint(runes []rune) (uint, int) {
	var n int
	var val uint

	for n < len(runes) && isDigit(runes[n]) {
		val = val*10 + uint(runes[n]-'0')
		n++
	}

	return val, n
}

// isDigit 判断是否是数字
func isDigit(ch rune) bool {
	return ch >= '0' && ch <= '9'
}

// isComplexOperand 判断是否是复合操作数格式
func isComplexOperand(runes []rune) bool {
	if len(runes) < 3 {
		return false
	}

	// 检查是否包含k后跟数字的格式
	for i, r := range runes {
		if r == 'k' && i+1 < len(runes) && isDigit(runes[i+1]) {
			return true
		}
	}
	return false
}

// parseComplexOperand 解析复合操作数
func parseComplexOperand(runes []rune) (*Arg, int, error) {
	// 查找k字符位置
	kPos := -1
	for i, r := range runes {
		if r == 'k' && i+1 < len(runes) && isDigit(runes[i+1]) {
			kPos = i
			break
		}
	}

	if kPos < 0 {
		return nil, 0, fmt.Errorf("invalid complex operand format")
	}

	// 解析k后面的数字作为位宽
	widthStr := ""
	for i := kPos + 1; i < len(runes); i++ {
		if isDigit(runes[i]) {
			widthStr += string(runes[i])
		} else {
			break
		}
	}

	if widthStr == "" {
		return nil, 0, fmt.Errorf("missing width in complex operand")
	}

	width, _ := strconv.Atoi(widthStr)

	// 解析前缀字符确定偏移量
	offset := uint(0)
	for i := 0; i < kPos; i++ {
		switch runes[i] {
		case 'd':
			offset = 0
		case 'j':
			offset = 5
		case 'k':
			offset = 10
		case 'a':
			offset = 15
		case 'm':
			offset = 16
		case 'n':
			offset = 18
		}
	}

	return &Arg{
		Kind: ArgKindComplex,
		Slots: []*Slot{{
			Offset: offset,
			Width:  uint(width),
		}},
	}, len(runes), nil
}

// generateOpmapFile 生成opmap.rs文件
func generateOpmapFile(path string, insns []*InsnDescription) error {
	file, err := os.Create(path)
	if err != nil {
		return err
	}
	defer file.Close()

	// 写入文件头
	file.WriteString("// This file was generated by tools/loongarch_gen_opmap\n")
	file.WriteString("Ops!(\n\n")

	// 按助记符分组
	insnMap := make(map[string][]*InsnDescription)
	for _, insn := range insns {
		insnMap[insn.Mnemonic] = append(insnMap[insn.Mnemonic], insn)
	}

	// 按助记符排序
	var mnemonics []string
	for m := range insnMap {
		mnemonics = append(mnemonics, m)
	}
	sort.Strings(mnemonics)

	// 生成每条指令的条目
	for _, mnemonic := range mnemonics {
		group := insnMap[mnemonic]
		file.WriteString(fmt.Sprintf("\"%s\" = [\n", mnemonic))

		// 检查是否需要显示原始名称
		showOrigName := false
		for _, insn := range group {
			if origName, ok := insn.Attribs["orig_name"]; ok && origName != mnemonic {
				showOrigName = true
				break
			}
		}

		// 生成每个编码变体
		for _, insn := range group {
			if showOrigName {
				if origName, ok := insn.Attribs["orig_name"]; ok {
					file.WriteString(fmt.Sprintf("    // orig_name: %s\n", origName))
				}
			}

			// 格式化编码为0bxxxx_xxxx_xxxx_xxxx，确保32位
			bits := fmt.Sprintf("%032b", insn.Word)
			if len(bits) != 32 {
				return fmt.Errorf("invalid instruction word length: %d", len(bits))
			}
			formattedBits := fmt.Sprintf("0b%s_%s_%s_%s",
				bits[0:8], bits[8:16], bits[16:24], bits[24:32])

			// 生成操作数列表
			var argList []string
			for _, arg := range insn.Format.Args {
				argList = append(argList, argToRust(arg))
			}

			// 生成处理器列表
			var procList []string
			for _, arg := range insn.Format.Args {
				procList = append(procList, argToProcessor(arg))
			}

			file.WriteString(fmt.Sprintf("    %s = [%s] => [%s];\n",
				formattedBits, strings.Join(argList, ", "), strings.Join(procList, ", ")))
		}

		file.WriteString("],\n\n")
	}

	file.WriteString(")\n")
	return nil
}

// argToRust 将操作数转换为Rust宏参数
func argToRust(arg *Arg) string {
	if arg == nil {
		return "Unknown"
	}

	switch arg.Kind {
	case ArgKindIntReg:
		return "D"
	case ArgKindFPReg:
		return "F"
	case ArgKindFCCReg:
		return "C"
	default:
		if len(arg.Slots) == 0 {
			return "Unknown"
		}
		if len(arg.Slots) == 1 {
			// 简单立即数
			prefix := "S"
			if arg.Kind == ArgKindUnsignedImm {
				prefix = "U"
			}
			return fmt.Sprintf("%sk%d", prefix, arg.Slots[0].Width)
		} else {
			// 复合立即数
			var parts []string
			for _, slot := range arg.Slots {
				parts = append(parts, fmt.Sprintf("k%d", slot.Width))
			}
			return fmt.Sprintf("S%s", strings.Join(parts, ""))
		}
	}
}

// argToProcessor 将操作数转换为处理器表达式
func argToProcessor(arg *Arg) string {
	if arg == nil {
		return "Unknown"
	}
	if len(arg.Slots) == 0 {
		return "Unknown"
	}

	switch arg.Kind {
	case ArgKindIntReg, ArgKindFPReg, ArgKindFCCReg:
		return fmt.Sprintf("R(%d)", arg.Slots[0].Offset)
	case ArgKindSignedImm:
		if len(arg.Slots) == 1 {
			return fmt.Sprintf("Sbits(%d, %d)", arg.Slots[0].Offset, arg.Slots[0].Width)
		} else {
			var parts []string
			for _, slot := range arg.Slots {
				parts = append(parts, fmt.Sprintf("(%d, %d)", slot.Offset, slot.Width))
			}
			return fmt.Sprintf("Sfields(&[%s])", strings.Join(parts, ", "))
		}
	case ArgKindUnsignedImm:
		if len(arg.Slots) == 1 {
			return fmt.Sprintf("Ubits(%d, %d)", arg.Slots[0].Offset, arg.Slots[0].Width)
		} else {
			var parts []string
			for _, slot := range arg.Slots {
				parts = append(parts, fmt.Sprintf("(%d, %d)", slot.Offset, slot.Width))
			}
			return fmt.Sprintf("Ufields(&[%s])", strings.Join(parts, ", "))
		}
	default:
		return "Unknown"
	}
}
