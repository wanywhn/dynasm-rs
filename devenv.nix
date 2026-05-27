{ pkgs, lib, config, inputs, ... }:

let
  crossPkgs = pkgs.pkgsCross.loongarch64-linux;
  loongarchSysroot = "${crossPkgs.stdenv.cc.libc}";
in
{
  # https://devenv.sh/languages/
  languages.rust = {
    enable = true;
    channel = "stable";
    targets = [ "loongarch64-unknown-linux-gnu" ];
  };

  # https://devenv.sh/packages/
  packages = with pkgs; [
    go
    python3
    qemu
    crossPkgs.binutils
    crossPkgs.stdenv.cc
  ];

  # https://devenv.sh/basics/
  env = {
    QEMU_LOONGARCH64 = "qemu-loongarch64";
    CARGO_TARGET_LOONGARCH64_UNKNOWN_LINUX_GNU_LINKER =
      "${crossPkgs.stdenv.cc}/bin/loongarch64-unknown-linux-gnu-gcc";
    # Run loongarch64 binaries via qemu-loongarch64 user-mode emulation.
    # -L sets the sysroot so the dynamic linker can find libc.
    CARGO_TARGET_LOONGARCH64_UNKNOWN_LINUX_GNU_RUNNER =
      "qemu-loongarch64 -L ${loongarchSysroot}";
  };

  enterShell = ''
    echo "dynasm-rs dev environment"
    echo "qemu-loongarch64: $(which qemu-loongarch64)"
    echo "cross-as: $(which loongarch64-unknown-linux-gnu-as)"
    echo "cross-gcc: $(which loongarch64-unknown-linux-gnu-gcc)"
    echo "loongarch sysroot: ${loongarchSysroot}"
  '';

  # https://devenv.sh/scripts/
  scripts = {
    gen-opmap.exec = ''
      cd tools/loongarch_gen_opmap && go build && cd -
      ./tools/loongarch_gen_opmap/loongarch_gen_opmap tools/loongarch_data/loongarch_opcodes plugin/src/arch/loongarch/opmap.rs
    '';
    # NOTE: Use --test loongarch_N (not bare "loongarch") to select test binaries by filename.
    # "cargo test loongarch" filters by test function name, which matches ZERO loongarch tests
    # since their function names are like add_d_0, sub_d_1, etc. — none contain "loongarch".
    test-loongarch.exec = ''
      cd testing && cargo test -j 1 --test loongarch_0 --test loongarch_1 --test loongarch_2 --test loongarch_3 --test loongarch_4 --test loongarch_5 --test loongarch_6
    '';
    # Cross-compile and run a loongarch64 binary via qemu.
    # Usage: run-loongarch <binary-path> [args...]
    # Example: run-loongarch ./target/loongarch64-unknown-linux-gnu/debug/my-program
    run-loongarch.exec = ''
      qemu-loongarch64 -L ${loongarchSysroot} "$@"
    '';
  };
}
