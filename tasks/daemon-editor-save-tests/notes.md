# Integration tests for vim, VS Code and sed -i save patterns (plan M3)

Plan M3 acceptance, second bullet. The three patterns exercise different watcher paths: rename
over the target (vim, sed), truncate-then-write with a visible partial state (VS Code).

## What each editor actually does
- **vim** (`backupcopy=auto`, default): keeps `.todo.txt.swp` while editing; on `:w` it writes the
  new bytes to a temp file in the same directory and renames it over the original (new inode).
  Ref: `:help backupcopy` https://vimhelp.org/options.txt.html#'backupcopy'
- **VS Code**: opens the existing file with truncate and writes in place, so a watcher can observe
  a 0-byte or partial file between events. Ref: https://github.com/microsoft/vscode/issues/94914
- **GNU/BSD `sed -i`**: writes `./sedXXXXXX`, then renames it over the target. Ref:
  https://www.gnu.org/software/sed/manual/sed.html#index-_002di

The tests reproduce the syscall sequence with `std::fs` rather than invoking the editors, so they
run on CI without vim or VS Code; `sed -i` is also run for real on unix as a second data point.

## Assertions specific to this suite
- The VS Code half-write must not generate ops: with a 50 ms gap inside a 150 ms debounce the
  actor sees only the final bytes. If the gap were > 150 ms the daemon *would* reconcile a
  truncated file and then reconcile again — correct, but two writes; that case is documented, not
  tested as passing.
- `.todo.txt.swp` and `sedXXXXXX` never appear in the daemon log as `reconcile{file}` targets.
