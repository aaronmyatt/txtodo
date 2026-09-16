//! Stamps a `service` field onto every formatted log line by wrapping the real writer, instead of
//! injecting the field through `tracing`'s span/field machinery (see the crate root doc and
//! `tasks/logging-telemetry-crate/notes.md`'s "Field name decision" section for the alternatives
//! considered and why this one won).
//!
//! Safety of the approach: `tracing-subscriber`'s `fmt` layer formats one whole event into a
//! thread-local buffer and issues exactly one `write_all` call per event —
//! <https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/>. That means `Write::write`
//! below always receives one complete, whole line, never a partial fragment — which is what makes
//! plain byte insertion (rather than a full parse/reserialize) correct here.

use std::io;

/// Which shape of line a [`StampWriter`] is stamping: full JSON objects (the rolling file layer)
/// or `tracing_subscriber`'s default compact `key=value` text (the pretty stderr layer).
#[derive(Clone, Copy)]
enum Kind {
    Json,
    Text,
}

/// Wraps a `MakeWriter`, handing out [`StampWriter`]s that stamp `service` onto every line the
/// inner writer would otherwise receive unmodified.
#[derive(Clone)]
struct StampMakeWriter<M> {
    inner: M,
    service: &'static str,
    kind: Kind,
}

impl<'a, M> tracing_subscriber::fmt::MakeWriter<'a> for StampMakeWriter<M>
where
    M: tracing_subscriber::fmt::MakeWriter<'a>,
{
    type Writer = StampWriter<M::Writer>;

    fn make_writer(&'a self) -> Self::Writer {
        StampWriter {
            inner: self.inner.make_writer(),
            service: self.service,
            kind: self.kind,
        }
    }
}

/// The actual `Write` wrapper — see the module doc for why this is safe to do with plain byte
/// insertion rather than a JSON parse/reserialize.
struct StampWriter<W> {
    inner: W,
    service: &'static str,
    kind: Kind,
}

impl<W: io::Write> io::Write for StampWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let stamped = match self.kind {
            Kind::Json => stamp_json(buf, self.service),
            Kind::Text => stamp_text(buf, self.service),
        };
        self.inner.write_all(&stamped)?;
        // Callers (tracing-subscriber's fmt layer) only check this against `buf.len()` to decide
        // whether the whole write "succeeded" — report the original length, not the stamped one,
        // so it never looks like a short write of the caller's own buffer.
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Inserts a flat, top-level `"service":"<name>",` key right after the first `{` — cheaper than a
/// full JSON parse/reserialize, and correct as long as `buf` is one complete JSON object (see
/// module doc). Falls back to passing `buf` through unmodified if no `{` is found (defensive only
/// — every line the JSON layer emits starts with one).
fn stamp_json(buf: &[u8], service: &str) -> Vec<u8> {
    match buf.iter().position(|&b| b == b'{') {
        Some(pos) => {
            let mut out = Vec::with_capacity(buf.len() + service.len() + 16);
            out.extend_from_slice(&buf[..=pos]);
            out.extend_from_slice(b"\"service\":");
            // `{service:?}` (Debug on &str) quotes/escapes exactly like serde_json would for any
            // input, not just plain ASCII service names — reused instead of hand-rolled quoting.
            out.extend_from_slice(format!("{service:?}").as_bytes());
            out.push(b',');
            out.extend_from_slice(&buf[pos + 1..]);
            out
        }
        None => buf.to_vec(),
    }
}

/// Appends ` service=<name>` before the trailing newline (or at the very end, if a line somehow
/// arrives without one) — the same `key=value` shape `tracing_subscriber`'s own compact formatter
/// already uses for every other field.
fn stamp_text(buf: &[u8], service: &str) -> Vec<u8> {
    let suffix = format!(" service={service}");
    if buf.ends_with(b"\n") {
        let mut out = Vec::with_capacity(buf.len() + suffix.len());
        out.extend_from_slice(&buf[..buf.len() - 1]);
        out.extend_from_slice(suffix.as_bytes());
        out.push(b'\n');
        out
    } else {
        let mut out = buf.to_vec();
        out.extend_from_slice(suffix.as_bytes());
        out
    }
}

/// Wraps `inner` so every line it receives gets a top-level `"service":"<name>",` JSON key.
pub(crate) fn json_writer<M>(
    inner: M,
    service: &'static str,
) -> impl for<'a> tracing_subscriber::fmt::MakeWriter<'a>
where
    M: for<'a> tracing_subscriber::fmt::MakeWriter<'a> + 'static,
{
    StampMakeWriter {
        inner,
        service,
        kind: Kind::Json,
    }
}

/// Wraps `inner` so every line it receives gets a trailing ` service=<name>` field.
pub(crate) fn text_writer<M>(
    inner: M,
    service: &'static str,
) -> impl for<'a> tracing_subscriber::fmt::MakeWriter<'a>
where
    M: for<'a> tracing_subscriber::fmt::MakeWriter<'a> + 'static,
{
    StampMakeWriter {
        inner,
        service,
        kind: Kind::Text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamp_json_inserts_flat_top_level_field() {
        let out = stamp_json(b"{\"level\":\"INFO\"}\n", "txtodod");
        let text = String::from_utf8(out).unwrap_or_else(|e| panic!("{e}"));
        assert!(
            text.starts_with("{\"service\":\"txtodod\",\"level\":\"INFO\"}"),
            "{text}"
        );
    }

    #[test]
    fn stamp_json_escapes_quotes_in_service_name() {
        let out = stamp_json(b"{}\n", "weird\"name");
        let text = String::from_utf8(out).unwrap_or_else(|e| panic!("{e}"));
        assert!(text.contains("\"service\":\"weird\\\"name\""), "{text}");
    }

    #[test]
    fn stamp_text_appends_before_trailing_newline() {
        let out = stamp_text(b"INFO starting\n", "relay");
        assert_eq!(out, b"INFO starting service=relay\n");
    }

    #[test]
    fn stamp_text_appends_when_no_trailing_newline() {
        let out = stamp_text(b"INFO starting", "relay");
        assert_eq!(out, b"INFO starting service=relay");
    }
}
