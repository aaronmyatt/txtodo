# cli-workspace-autoregister

## Summary

First `add`/`init` in a directory with a `todo.txt` auto-registers it as a workspace, with no
separate onboarding step.

## As built

Already delivered by `cli-workspace-commands` (`a52c586`): every daemon-mode RPC, starting with
`run_via_daemon`'s first `list_files` call, carries a real `Path` selector that auto-registers on
any call. No new production code was needed; added regression coverage against a completely fresh
directory instead (`7971c0b`).
