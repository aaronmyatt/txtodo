//! The `clap` argument grammar: `Cli` (global flags) and `Command` (every subcommand). Split out
//! of `main.rs` for the file budget; `main.rs` keeps `run`/`dispatch`/`dispatch_daemon`.

use crate::bundle;
use crate::commands;
use clap::{Parser, Subcommand};

/// todo.sh-compatible todo.txt tool. Commands and aliases match todo.sh; line numbers are the ids.
#[derive(Debug, Parser)]
#[command(
    name = "txtodo",
    version = crate::buildinfo::VERSION_LINE,
    about,
    before_long_help = crate::logo::LOGO
)]
pub struct Cli {
    /// Todo directory (overrides $TXTODO_TODO_DIR and config `todo_dir`).
    #[arg(long, global = true, value_name = "DIR")]
    pub dir: Option<String>,
    /// File-carrier sync folder (overrides $TXTODO_SYNC_DIR and config `sync_dir`; plan M8
    /// `sync-file-carrier`). Unset means file-carrier sync is not configured.
    #[arg(long, global = true, value_name = "DIR")]
    pub sync_dir: Option<String>,
    /// Relay URL (overrides $TXTODO_RELAY_URL and config `relay_url`; plan M8
    /// `sync-relay-enable`, ADR 0026). Unset means relay stays off — LAN-only, unchanged.
    #[arg(long, global = true, value_name = "URL")]
    pub relay: Option<String>,
    /// Emit one JSON object per line on listing commands.
    #[arg(long, global = true)]
    pub json: bool,
    /// Do not stamp `id:` on added tasks (overrides config `id_tags`).
    #[arg(long, global = true)]
    pub no_id: bool,
    /// Leave a line where it is after `do`, instead of moving it to the end (todo.sh -A).
    #[arg(short = 'A', long, global = true)]
    pub no_archive: bool,
    /// Ignore a running daemon and edit the files directly (M2 behaviour).
    #[arg(long, global = true)]
    pub no_daemon: bool,
    /// Run the command on this named list (see `txtodo lists`) instead of the workspace here:
    /// the name it shows, or the name it was made under. Asks the daemon where the list lives.
    #[arg(long, global = true, value_name = "NAME", conflicts_with = "dir")]
    pub list: Option<String>,
    /// A list inside the workspace, relative to its root, to run the command on instead of the
    /// root list. `sub` passes it, so a sub-list is worked as one more list of its workspace,
    /// never registered as a workspace of its own (sync-drift line 3). Hidden: `sub ITEM#` is
    /// the way to name one. Was `--list` before named lists took that name (task
    /// two-kinds-of-workspace). Ref: <https://docs.rs/clap/latest/clap/struct.Arg.html#method.hide>
    #[arg(long, global = true, value_name = "FILE", hide = true)]
    pub sublist: Option<String>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Add a task: today's date after the priority, then an `id:` tag.
    #[command(visible_alias = "a")]
    Add {
        /// The task; several words are joined with spaces.
        #[arg(required = true, num_args = 1..)]
        text: Vec<String>,
    },
    /// Add several tasks, one per line of TEXT.
    Addm {
        /// The tasks; each line becomes one task.
        #[arg(required = true, num_args = 1..)]
        text: Vec<String>,
    },
    /// Add text to the end of a task.
    #[command(visible_alias = "app")]
    Append {
        /// Line number.
        item: String,
        /// Text to append; several words are joined with spaces.
        #[arg(required = true, num_args = 1..)]
        text: Vec<String>,
    },
    /// Move every completed line to the bottom of the file and drop blank lines: the explicit
    /// full sort. `do` alone only moves the line it completed.
    Archive,
    /// Remove a task's priority.
    #[command(visible_alias = "dp")]
    Depri {
        /// Line numbers, comma or space separated.
        #[arg(required = true, num_args = 1..)]
        items: Vec<String>,
    },
    /// Blank every later repeat of an identical line.
    Deduplicate,
    /// Delete a task (its line stays blank), or remove TERM from it.
    #[command(visible_alias = "rm")]
    Del {
        /// Line number.
        item: String,
        /// Text to remove from the line instead of deleting it.
        term: Option<String>,
    },
    /// Mark tasks done: `x`, today's date, priority kept as `pri:`; then move them to the end of
    /// the file (`-A` leaves them in place). Other done lines and blank lines are not touched.
    Do {
        /// Line numbers, comma or space separated.
        #[arg(required = true, num_args = 1..)]
        items: Vec<String>,
    },
    /// Print the resolved paths and config.
    Env,
    /// Canonicalise quirks in todo.txt (the only command that rewrites untouched lines).
    Fmt,
    /// Report quirks and file hygiene in todo.txt.
    Lint,
    /// List open tasks matching every TERM (`-term` excludes), sorted. Done tasks show only when a
    /// term asks for them (`is:done`); `listall` shows every line.
    #[command(visible_alias = "ls")]
    List {
        /// Search terms; `-term` excludes.
        #[arg(allow_hyphen_values = true)]
        terms: Vec<String>,
    },
    /// List every task in todo.txt, done tasks included.
    #[command(visible_alias = "lsa")]
    Listall {
        /// Search terms; `-term` excludes.
        #[arg(allow_hyphen_values = true)]
        terms: Vec<String>,
    },
    /// List tasks with a priority, optionally only `A` or a range `A-C`.
    #[command(visible_alias = "lsp")]
    Listpri {
        /// An optional priority or range, then search terms.
        #[arg(allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// List the projects (`+word`) of matching tasks.
    #[command(visible_alias = "lsprj")]
    Listproj {
        /// Search terms; `-term` excludes.
        #[arg(allow_hyphen_values = true)]
        terms: Vec<String>,
    },
    /// List the contexts (`@word`) of matching tasks.
    #[command(visible_alias = "lsc")]
    Listcon {
        /// Search terms; `-term` excludes.
        #[arg(allow_hyphen_values = true)]
        terms: Vec<String>,
    },
    /// Add text after a task's priority and date.
    #[command(visible_alias = "prep")]
    Prepend {
        /// Line number.
        item: String,
        /// Text to prepend; several words are joined with spaces.
        #[arg(required = true, num_args = 1..)]
        text: Vec<String>,
    },
    /// Move a task to another list in the todo directory. Through the daemon its `ref:` folder
    /// moves too; without one, its line stays blank (todo.sh).
    #[command(visible_alias = "mv")]
    Move {
        /// Line number.
        item: String,
        /// Destination file name, relative to the todo directory (e.g. `tasks/trip/todo.txt`).
        dest: String,
        /// Source file name (default todo.txt).
        src: Option<String>,
    },
    /// Copy a task to the end of another list: a new task without the `id:` and `ref:` (its notes
    /// and sub-list stay with the original).
    #[command(visible_alias = "cp")]
    Copy {
        /// Line number.
        item: String,
        /// Destination file name, relative to the todo directory.
        dest: String,
        /// Source file name (default todo.txt).
        src: Option<String>,
    },
    /// Set task priorities: ITEM# PRIORITY pairs, A to Z.
    #[command(visible_alias = "p")]
    Pri {
        /// `ITEM# PRIORITY` pairs.
        #[arg(required = true, num_args = 2..)]
        args: Vec<String>,
    },
    /// Archive, then record the task and done counts in report.txt.
    Report,
    /// Replace a task's text, keeping its priority and date.
    Replace {
        /// Line number.
        item: String,
        /// The new text; several words are joined with spaces.
        #[arg(required = true, num_args = 1..)]
        text: Vec<String>,
    },
    /// List the `.txt` files in the todo directory, or the tasks in FILE.
    #[command(visible_alias = "lf")]
    Listfile {
        /// A file name (looked up in the todo directory) then search terms.
        #[arg(allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Show the op log, newest first (daemon mode).
    Log {
        /// Only this document (workspace-relative), e.g. q4/todo.txt.
        #[arg(long)]
        file: Option<String>,
        /// How many ops.
        #[arg(short = 'n', long)]
        n: Option<u32>,
    },
    /// Who last touched each field of a task (daemon mode).
    Blame {
        /// Line number.
        item: String,
    },
    /// Undo the newest ops (daemon mode).
    Undo {
        /// How many ops to invert.
        #[arg(long, default_value_t = 1)]
        steps: u32,
    },
    /// Render a document as it was at a local date-time (daemon mode).
    Checkout {
        /// YYYY-MM-DDTHH:MM[:SS] in the local zone.
        at: String,
        /// Print to stdout instead of a temp file.
        #[arg(long)]
        stdout: bool,
        /// Which document; the workspace's root list by default.
        #[arg(long)]
        file: Option<String>,
    },
    /// Duplicate lines (daemon mode): `list` (default), `delete <line>` or `keep-newest`.
    Conflicts {
        #[command(subcommand)]
        action: Option<commands::conflicts::Action>,
    },
    /// Devices paired into this workspace's sync group (daemon mode): `list` (default) or
    /// `remove <id>`.
    Device {
        #[command(subcommand)]
        action: Option<commands::device::Action>,
    },
    /// Identity of tasks across edits (daemon mode): `migrate` strips `id:` tags from a workspace
    /// that still has them (ADR 0019).
    Identity {
        #[command(subcommand)]
        action: commands::identity::Action,
    },
    /// Pair with another device: no CODE starts a handshake and shows a QR/code; CODE (scanned or
    /// pasted from the other device) joins it and shows the six-word SAS to compare (daemon mode,
    /// plan M4, design §4).
    Pair {
        /// The other device's offer (from its QR or `txtodo pair`'s own printed text).
        code: Option<String>,
    },
    /// Check socket, watcher, files, clock and config; exit 1 on any failure.
    Doctor {
        /// Also print the daemon's recent JSON log.
        #[arg(long)]
        verbose: bool,
    },
    /// Manage the txtodod service for this workspace (launchd on macOS, systemd --user on Linux).
    Daemon {
        /// What to do.
        action: commands::service::Action,
        /// Overwrite an existing service file on install.
        #[arg(long)]
        force: bool,
    },
    /// Serve the Model Context Protocol surface for this workspace (design §6.1).
    Mcp {
        /// Serve over stdin/stdout.
        #[arg(long)]
        stdio: bool,
        /// Serve Streamable HTTP on 127.0.0.1:8636/mcp. Loopback only; no flag widens it.
        #[arg(long)]
        http: bool,
        /// Attached to every mutation as the agent principal.
        #[arg(long)]
        token: Option<String>,
    },
    /// Prints the resolved `ref:` directory for a line; never creates it (daemon mode, rules 2, 9).
    Open {
        /// Line number in todo.txt.
        item: String,
    },
    /// Opens `$EDITOR` on a line's `ref:`/notes.md, creating the directory lazily (daemon mode,
    /// rule 4).
    Notes {
        /// Line number in todo.txt.
        item: String,
    },
    /// Runs COMMAND with its directory scoped to a line's `ref:` sub-list (daemon mode, rule 12);
    /// a scoped `todo.sh -d`.
    Sub {
        /// Line number in todo.txt, or a `ref:` slug path: `trip`, nested `trip/flights`.
        item: String,
        /// The command (and its own arguments) to run inside that directory.
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        cmd: Vec<String>,
    },
    /// Moves the whole workspace as one file (daemon mode, plan M8, design §4.5): `export` writes
    /// it, `import` reads it back — no network involved either way (air-gapped sneakernet
    /// carrier).
    Bundle {
        #[command(subcommand)]
        action: bundle::Action,
    },
    /// Manages the device-global daemon's workspace registry (ADR 0025): `list` (default),
    /// `add [DIR]`, `remove <id>`; and a peer's workspace offers: `offers`, `accept`, `decline`.
    Workspace {
        #[command(subcommand)]
        action: Option<commands::workspace::Action>,
    },
    /// Named lists that txtodo syncs between your own devices: `new NAME`, `ls` (default),
    /// `rename LIST NEW`, `rm LIST`, `import DIR [NAME]`. Work on one with `--list NAME`.
    Lists {
        #[command(subcommand)]
        action: Option<commands::lists::Action>,
    },
    /// The agent playbook for working this backlog: a Claude Code Skill under the user's global
    /// `~/.claude/skills`, and a marked section in this project's `AGENTS.md` (cross-agent).
    Skill {
        #[command(subcommand)]
        action: commands::skill::Action,
    },
    /// The daemon's OS notice for changes from another device (ADR 0041): `on`, `off`, `status`.
    Notices {
        #[command(subcommand)]
        action: commands::notices::Action,
    },
    /// Upgrades to the newest release the way this copy was installed (Homebrew, the install
    /// script, or a source build), then restarts the daemon. `--check` only says what it would do.
    Upgrade {
        /// Print this build, the newest release and the upgrade command; change nothing.
        #[arg(long)]
        check: bool,
    },
    /// The git merge driver for todo lists (task two-kinds-of-workspace): merges the file git calls
    /// `%A` with `%B`, three ways against `%O`, line by line and without losing a line, and writes
    /// the result into `%A`. `txtodo git setup` installs it; it is not meant to be typed.
    #[command(name = "merge-driver")]
    MergeDriver {
        /// The common ancestor (git's `%O`).
        base: String,
        /// Our version (git's `%A`); the merge is written here.
        ours: String,
        /// Their version (git's `%B`).
        theirs: String,
    },
    /// Lists `ref:` directories no line points to; deletes them only with `--yes` (daemon mode,
    /// rule 10).
    Prune {
        /// The only supported prune mode today.
        #[arg(long)]
        orphans: bool,
        /// Actually delete what would otherwise only be listed.
        #[arg(long)]
        yes: bool,
    },
}
