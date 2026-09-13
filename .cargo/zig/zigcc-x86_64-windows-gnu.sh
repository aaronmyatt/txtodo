#!/bin/sh
# zig cc as the linker for x86_64-pc-windows-gnu — zig ships mingw-w64 import libs, so this
# needs no MSVC and no separate mingw toolchain install on the (Linux) CI runner.
# Cargo `linker` needs one bare executable, hence this wrapper:
# https://doc.rust-lang.org/cargo/reference/config.html#target
exec zig cc -target x86_64-windows-gnu "$@"
