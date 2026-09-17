# cli-doctor-multi-workspace

## Summary

`txtodo doctor` reports every registered workspace's health in one run instead of one per cwd.

## As built

The seven fixed checks stay exactly as-is for the cwd's own workspace (script index compat); one
lightweight "workspace" row per other registered entry (id, root, doc count or error) is appended
after the peer rows. `daemon_checks` keeps the live connection alive (`DaemonState`) so
`Daemon::health_for_id` can probe other workspace ids without reconnecting (`e11b374`).
