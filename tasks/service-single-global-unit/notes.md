# service-single-global-unit

## Summary

One launchd/systemd `--user` unit total for the whole device, not one per workspace hash;
`txtodo daemon install` migrates any existing per-workspace units.

## As built

Templates drop `--dir`/`{{WORKSPACE}}` entirely (true global mode); the label collapses from a
per-workspace hash suffix to one bare `com.txtodo.txtodod`. `install` scans for and removes any
pre-M11 per-workspace unit files, best-effort stop then delete, never fatal to the install
(`150821b`).
