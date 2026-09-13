#!/bin/sh
# zig cc as the linker for x86_64-unknown-linux-gnu. Pins an old glibc (2.17, RHEL7-era) for
# portability, mirroring cargo-zigbuild's own glibc-version suffix convention:
# https://github.com/rust-cross/cargo-zigbuild#specify-glibc-version
# Cargo `linker` needs one bare executable, hence this wrapper:
# https://doc.rust-lang.org/cargo/reference/config.html#target
exec zig cc -target x86_64-linux-gnu.2.17 "$@"
