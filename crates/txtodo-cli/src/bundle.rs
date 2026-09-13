//! `txtodo bundle export|import` (plan M8 `cli-bundle`, design §4.5): the CLI half — the command
//! surface, the passphrase prompt, and a bounded copy loop between the local file and the
//! daemon's gRPC stream (`client.rs`'s `bundle_export`/`bundle_import`). All crypto and store
//! access live in the daemon (`crates/txtodo-daemon/src/bundle_export.rs`/`bundle_import.rs`) —
//! this crate may not depend on txtodo-store/txtodo-sync (`check-boundaries.sh`), so it never
//! touches either directly.
//!
//! On-disk framing: the bundle file is a sequence of `[u32 LE length][length bytes]` frames, each
//! one exactly one `BundleChunk.data` payload from the daemon's stream, in order — never
//! re-chunked to an I/O buffer's own size. That is what lets this copy loop stay bounded (each
//! frame is already capped by the daemon at a modest size) while still reproducing the exact
//! chunk boundaries the encrypted body's STREAM construction needs on the way back in.

use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Write};

use clap::Subcommand;
use txtodo_proto::v1 as pb;

use crate::CliError;
use crate::client::Daemon;

/// Sanity cap on one on-disk frame this CLI will read back — defends a truncated/hostile file
/// from an unbounded allocation; comfortably above anything the daemon actually emits.
const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

/// gRPC request metadata key carrying `BundleImport`'s passphrase — must match
/// `txtodo-daemon/src/bundle_grpc.rs`'s own constant of the same name exactly.
const PASSPHRASE_METADATA_KEY: &str = "x-txtodo-bundle-passphrase-bin";

/// The `txtodo bundle` subcommands.
#[derive(Debug, Subcommand)]
pub enum Action {
    /// Writes the whole workspace (every file, plus the full op log) to one encrypted file.
    Export {
        /// Output file (default `bundle.txtodo`).
        #[arg(long)]
        out: Option<String>,
        /// Read the passphrase from this file, or `-` for stdin; omitted prompts interactively.
        #[arg(long)]
        passphrase_file: Option<String>,
    },
    /// Reads a bundle back in: verified, then applied — all-or-nothing.
    Import {
        /// The bundle file to read.
        file: String,
        /// Read the passphrase from this file, or `-` for stdin; omitted prompts interactively.
        #[arg(long)]
        passphrase_file: Option<String>,
    },
}

/// Entry: `txtodo bundle export|import` (daemon mode only — the seam is the daemon's gRPC
/// socket, never a direct store read; design §4.5's own rule).
pub fn run(daemon: &mut Daemon, action: &Action) -> Result<(), CliError> {
    match action {
        Action::Export {
            out,
            passphrase_file,
        } => run_export(daemon, out.as_deref(), passphrase_file.as_deref()),
        Action::Import {
            file,
            passphrase_file,
        } => run_import(daemon, file, passphrase_file.as_deref()),
    }
}

fn run_export(
    daemon: &mut Daemon,
    out: Option<&str>,
    passphrase_file: Option<&str>,
) -> Result<(), CliError> {
    let passphrase = read_passphrase(passphrase_file, "txtodo: bundle export passphrase: ")?;
    let out_path = out.unwrap_or("bundle.txtodo");
    let mut w = BufWriter::new(File::create(out_path)?);
    daemon.bundle_export(passphrase, |data| write_frame(&mut w, data))?;
    w.flush()?;
    println!("txtodo: wrote {out_path}");
    Ok(())
}

fn run_import(
    daemon: &mut Daemon,
    file: &str,
    passphrase_file: Option<&str>,
) -> Result<(), CliError> {
    let passphrase = read_passphrase(passphrase_file, "txtodo: bundle import passphrase: ")?;
    let mut r = BufReader::new(File::open(file)?);
    let rep = daemon.bundle_import(passphrase, move || read_frame(&mut r))?;
    println!(
        "txtodo: imported {} ops across {} files",
        rep.ops_imported,
        rep.files.len()
    );
    Ok(())
}

fn write_frame(w: &mut impl Write, data: &[u8]) -> io::Result<()> {
    let len = u32::try_from(data.len()).map_err(|_| io::Error::other("frame too large"))?;
    w.write_all(&len.to_le_bytes())?;
    w.write_all(data)
}

fn read_frame(r: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut len_bytes = [0u8; 4];
    match r.read_exact(&mut len_bytes) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes(len_bytes) as usize;
    if len > MAX_FRAME_BYTES {
        return Err(io::Error::other(format!(
            "frame of {len} bytes over the {MAX_FRAME_BYTES} cap"
        )));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(Some(buf))
}

/// Reads a passphrase: `-` means one line from stdin, a path reads and trims that file's bytes,
/// `None` prompts interactively on stderr — never a CLI argument or environment variable
/// (CLAUDE.md §3.1), the same discipline `txtodod`'s own `prompt_file_passphrase` uses.
fn read_passphrase(source: Option<&str>, prompt: &str) -> Result<Vec<u8>, CliError> {
    match source {
        Some("-") => read_stdin_line(),
        Some(path) => Ok(trim_trailing_newline(std::fs::read(path)?)),
        None => {
            eprint!("{prompt}");
            io::stderr().flush().ok();
            read_stdin_line()
        }
    }
}

fn read_stdin_line() -> Result<Vec<u8>, CliError> {
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    Ok(trim_trailing_newline(line.into_bytes()))
}

fn trim_trailing_newline(mut bytes: Vec<u8>) -> Vec<u8> {
    while matches!(bytes.last(), Some(b'\n') | Some(b'\r')) {
        bytes.pop();
    }
    bytes
}

/// Sets `BundleImport`'s request metadata to `passphrase` — split out here (rather than
/// `client.rs`) purely for that file's own line budget; called from `Daemon::bundle_import`.
pub(crate) fn insert_passphrase<T>(req: &mut tonic::Request<T>, passphrase: &[u8]) {
    let value = tonic::metadata::MetadataValue::from_bytes(passphrase);
    req.metadata_mut()
        .insert_bin(PASSPHRASE_METADATA_KEY, value);
}

/// Reads frames off `next_frame` and sends each into `tx`, until end of file or the server hangs
/// up. Runs as its own task so it can run concurrently with the RPC awaiting the response
/// (`Daemon::bundle_import`'s own doc).
pub(crate) async fn drain_frames(
    mut next_frame: impl FnMut() -> io::Result<Option<Vec<u8>>> + Send + 'static,
    tx: tokio::sync::mpsc::Sender<pb::BundleChunk>,
) -> io::Result<()> {
    loop {
        match next_frame()? {
            Some(data) => {
                if tx.send(pb::BundleChunk { data }).await.is_err() {
                    return Ok(()); // the server closed the stream early
                }
            }
            None => return Ok(()),
        }
    }
}
