# mcp-cwd-autoregister

Found reviewing the last 100 commits (`18b37ea^..HEAD`) on 2026-09-21.

## Goal

Before this range the MCP server required an explicit `--dir` or `--global`. `e7b48b8` made it
serve the current folder if it is a workspace, else the default. The convenience is right; the
side effect is that starting the server anywhere there happens to be a `todo.txt` permanently
registers that folder and announces its name to every paired peer.

## The chain

1. `crates/txtodo-workspace-paths/src/lib.rs:227-231` — `is_workspace_dir` treats a bare `todo.txt`
   as a workspace.
2. `crates/txtodo-mcp/src/main.rs:149-176` — `Target::Auto` then sends that folder as a `path`
   selector on every call.
3. `crates/txtodo-daemon/src/control_session.rs:107-122` → `workspace_catalog.rs:196-205` —
   `resolve_inner` auto-registers unknown paths via `open_one` → `registry.add`.
4. `outbound_offers` announces **every** registered workspace's folder name to every paired peer.

So an MCP server started in `~/clients/client-acme-nda/` leaks that directory name, with no user
action. This is the same class as the absolute-path audit `6d450cf` ran — that one confirmed no
*absolute* path escapes, and this is a folder *name* escaping instead.

## Related: two definitions of "a workspace"

`workspace_paths::workspace_root_from` (`lib.rs:185-196`) only climbs to a `.txtodo/` directory,
while `is_workspace_dir` (`:227-243`) also accepts `txtodo.toml` or `todo.txt`. In a workspace no
daemon has opened yet there is no `.txtodo/`, so `cd tasks/plan && txtodo add …` sees
`tasks/plan/todo.txt`, calls that folder the workspace, and registers the sub-directory — precisely
the "registers `tasks/<slug>/` as a workspace of its own" failure the function's own doc says it
exists to prevent. From a sub-folder with no `todo.txt` the same command silently lands in the
default workspace instead of the workspace above it.

One definition, used by both, is the fix. Which one it should be is a real call: `.txtodo/`-only is
safe but means a fresh checkout is not a workspace until a daemon touches it.

## Design sketch

Auto-resolution should be allowed to *serve* a folder without *registering* it. A path selector
that says "do not add this to the registry if it is unknown" separates the two, and keeps the
convenience without the leak.

See [[layout-toml-validation]] (the other place two definitions drifted).

## As built (2026-09-23)

- The leak is closed at the MCP side: with no flags, the server serves the folder it runs in only
  when the daemon's registry already lists it (registered earlier by the CLI/desktop); a folder
  the daemon does not know is not registered by the server and the default workspace is served,
  with a stderr line telling the human how to register it. The "serve without registering"
  selector flag from the design sketch was not built: it needs a proto field and catalog support
  for ephemeral opens, and the registered-only rule keeps the convenience for every workspace a
  person has actually used.
- One definition of the root: `workspace_root_from` prefers the nearest `.txtodo/`, else the
  farthest `todo.txt`/`txtodo.toml` folder before a `.git` boundary. Two nested, unrelated
  workspaces with no `.git` between them now resolve to the outer one — consistent with rule 11
  (every list under a root is the root's), and the case the old rule got wrong (a fresh
  checkout's `tasks/<slug>/`) is the common one.
