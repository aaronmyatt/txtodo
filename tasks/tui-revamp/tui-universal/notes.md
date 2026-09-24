# tui-universal

## Goal
- The c2 Universal page as a TUI screen.

## Design
- Data comes from the `UniversalTasks` RPC.
  - Grouping and due buckets use core `universal::*` against local today.
- Follow `apps/desktop/design-mockups/c2/universal.js`:
  - the stat strip and group selector
  - workspace chips (at least one stays on), show done, context chips
  - the row layout
  - the empty state with Reset filters
- Enter switches workspace, goes to Tasks and puts the cursor on that line.

## As built
