# Update plan §1 decision 10 and every other Sisyphus identifier

The project was renamed at /setup (crates `txtodo-*`, binaries `txtodo` / `txtodod` / `txtodo-tui`).
Both docs still carry the old names. Plan §1 #10 is normative ("fixed so docs and tests can rely on
them"), so it changes first; the rest follows.

## Rename map (ordered: longest patterns first so `sisd` does not eat `sisyphus-daemon`)
| From | To | Notes |
|---|---|---|
| `_sisyphus-mcp._tcp` | `_txtodo-mcp._tcp` | |
| `_sisyphus._udp` | `_txtodo._udp` | |
| `SISYPHUS_TODO_DIR` | `TXTODO_TODO_DIR` | |
| `sisyphus_` (metric prefix) | `txtodo_` | design §10 |
| `sisyphus-sync` (protocol name) | `txtodo-sync` | |
| `sisyphus-<crate>` | `txtodo-<crate>` | all 10 |
| `todotxt-core`, `todotxt-query` | `txtodo-core`, `txtodo-query` | crate names only |
| `.sisyphus/` | `.txtodo/` | state dir |
| `$XDG_CONFIG_HOME/sisyphus/` | `$XDG_CONFIG_HOME/txtodo/` | |
| `sisd` | `txtodod` | word-boundary match |
| `` `sis ` `` and `sis ` at line start in code blocks | `txtodo ` | the CLI; **not** the word "sis" in prose |
| `sisyphus-design.md`, `sisyphus-implementation-plan.md` | `txtodo-…` | cross-references |
| `Sisyphus` (project name in prose) | `txtodo` | keep the tagline and the "one must imagine…" epigraph |

## Do NOT rename
- `todotxt://` resource URIs (design §6.4): the scheme names the format, not the project.
- `todo.txt`, `done.txt`, `todo.sh`: the format and the reference CLI.
- `todotxt-core::tokenize` in prose → `txtodo_core::tokenize` (Rust path), not left as is.

## Suggested command shape
```bash
# One sed -E per pattern, longest first, on both files; then git diff --word-diff to review.
sed -i '' -E 's/_sisyphus-mcp\._tcp/_txtodo-mcp._tcp/g; s/_sisyphus\._udp/_txtodo._udp/g; …' txtodo-*.md
```
Both docs are plain files (not frozen). Re-read the diff before committing; sed cannot see prose context.
