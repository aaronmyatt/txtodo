# editor-save-lost

## Goal

A save from an editor is never overwritten by the daemon's next write.

## Evidence (2026-09-30, found by the p2p lab)

One device, no sync, the lab image (`txtodo-lab:src-cbee7b752392`, 0.0.19 code):

1. `txtodo add "cli-2 +lab"`
2. an editor-style save: copy `todo.txt`, append `editor-2`, write a temp file, rename it over
   `todo.txt`
3. `txtodo add "cli-3 +lab"` straight after

The file ends with `cli-1, editor-1, cli-2, cli-3`: `editor-2` is gone. The daemon logged no
`external_change` reconcile for it; its next `projection_written` wrote over it. A save with a
second or two of quiet around it (`editor-1`) survives.

In `lan-converge` (seed 42, both attempts) every token the no-loss check found missing came from
an editor-style save, none from a CLI edit. There the next write was an incoming sync op.

The lab's `lan-converge` reproduces it (`scripts/lab/lab.sh run lan-converge 42`).

## Open

- Not checked yet: whether the actor compares the file with the bytes it last wrote before it
  writes its projection, and why the rename's watcher event did not win.
- Whether it happens on macOS (FSEvents) too, or only under inotify in the container.
