//! What a `BundleChunk` carries once decrypted (plan M8 `cli-bundle`, design §4.5). Not protobuf —
//! this is the *inside* of the encrypted body, framed as length-prefixed postcard records so a
//! record boundary never has to line up with an encryption chunk boundary
//! (`bundle_crypto.rs` seals/opens fixed-size chunks; this module reassembles the logical record
//! stream across them).

use serde::{Deserialize, Serialize};
use txtodo_model::{Fingerprint, Op, TaskId};
use txtodo_sync::Signature;

/// One record's length prefix, bytes.
const LEN_PREFIX_BYTES: usize = 4;
/// Longest single record this reader accepts before refusing (defends a hostile/corrupt bundle
/// from an unbounded allocation) — the same ceiling `txtodo_store` already enforces per file.
pub(crate) const MAX_RECORD_BYTES: usize = txtodo_store::MAX_PROJECTION_BYTES;

/// One entry of the encrypted body: a document's exact bytes (the "snapshot"), one op-log row
/// with the exporting device's attestation signature (the "tail"), or one sidecar identity
/// fingerprint row. Fingerprints travel too because a sidecar-mode `recover()` on the importer
/// needs them to take the fast, no-reconcile path instead of minting brand-new ops for a document
/// it just received (see `bundle_import.rs`'s module doc) — empty for a tagged workspace.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) enum BundleTailRecord {
    /// A document's exact bytes at export time, workspace-relative path included.
    Blob {
        /// Workspace-relative path, `/` separators.
        path: String,
        /// The file's exact bytes.
        bytes: Vec<u8>,
    },
    /// One `ops` row, exactly as the exporting store held it, plus the exporting device's own
    /// Ed25519 signature over `Op::signing_bytes()` — verified against the manifest's
    /// `device_signing_public_key` before any insert (`bundle_import.rs`).
    Op {
        /// The op, unchanged: same id, HLC, principal, file, kind as the exporting store held.
        op: Op,
        /// The exporting device's Ed25519 signature over `op.signing_bytes()`.
        signature: Signature,
    },
    /// One sidecar identity fingerprint row (docs/questions.md Q2, `txtodo_store::FingerprintRow`).
    Fingerprint {
        /// Workspace-relative path this fingerprint belongs to.
        path: String,
        /// The task.
        task: TaskId,
        /// The fingerprint as of `updated_at_ms`.
        fingerprint: Fingerprint,
        /// Unix milliseconds this fingerprint was last computed.
        updated_at_ms: u64,
    },
}

impl BundleTailRecord {
    /// Encodes one record as `[len: u32 LE][postcard bytes]`, appended to `out`.
    pub(crate) fn encode_into(&self, out: &mut Vec<u8>) -> Result<(), postcard::Error> {
        let body = postcard::to_allocvec(self)?;
        debug_assert!(body.len() <= MAX_RECORD_BYTES, "one file/op at a time");
        let len = u32::try_from(body.len()).unwrap_or(u32::MAX);
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&body);
        Ok(())
    }
}

/// Why a record stream could not be read back.
#[derive(Debug)]
pub(crate) enum RecordReadError {
    /// A record claimed a length over [`MAX_RECORD_BYTES`].
    TooLarge(usize),
    /// The record's postcard bytes did not decode.
    Codec(postcard::Error),
    /// The stream ended with leftover bytes that never formed a whole record.
    Truncated(usize),
}

impl std::fmt::Display for RecordReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecordReadError::TooLarge(n) => write!(f, "record claims {n} bytes, over the cap"),
            RecordReadError::Codec(e) => write!(f, "record did not decode: {e}"),
            RecordReadError::Truncated(n) => write!(f, "{n} leftover bytes never formed a record"),
        }
    }
}

impl std::error::Error for RecordReadError {}

/// Reassembles [`BundleTailRecord`]s from a plaintext byte stream delivered in arbitrary-sized
/// pieces (one STREAM chunk's plaintext at a time). Bounded: `carry` never holds more than one
/// record's worth of leftover bytes plus the next chunk just fed in.
#[derive(Default)]
pub(crate) struct BundleRecordReader {
    carry: Vec<u8>,
}

impl BundleRecordReader {
    /// Feeds one chunk's plaintext in; returns every whole record it can now assemble, in order.
    pub(crate) fn feed(
        &mut self,
        plaintext: &[u8],
    ) -> Result<Vec<BundleTailRecord>, RecordReadError> {
        self.carry.extend_from_slice(plaintext);
        let mut out = Vec::new();
        while let Some(record) = self.try_take_one()? {
            out.push(record);
        }
        Ok(out)
    }

    /// Pulls one record off the front of `carry`, if a whole one is present.
    fn try_take_one(&mut self) -> Result<Option<BundleTailRecord>, RecordReadError> {
        if self.carry.len() < LEN_PREFIX_BYTES {
            return Ok(None);
        }
        let len = u32::from_le_bytes([self.carry[0], self.carry[1], self.carry[2], self.carry[3]])
            as usize;
        if len > MAX_RECORD_BYTES {
            return Err(RecordReadError::TooLarge(len));
        }
        if self.carry.len() < LEN_PREFIX_BYTES + len {
            return Ok(None);
        }
        let body = &self.carry[LEN_PREFIX_BYTES..LEN_PREFIX_BYTES + len];
        let record: BundleTailRecord =
            postcard::from_bytes(body).map_err(RecordReadError::Codec)?;
        self.carry.drain(..LEN_PREFIX_BYTES + len);
        Ok(Some(record))
    }

    /// Call once the underlying stream is exhausted: `Ok(())` only if nothing partial remains.
    pub(crate) fn finish(self) -> Result<(), RecordReadError> {
        if self.carry.is_empty() {
            Ok(())
        } else {
            Err(RecordReadError::Truncated(self.carry.len()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use txtodo_model::{DeviceId, Ulid};

    fn fp() -> Fingerprint {
        Fingerprint {
            creation_date: None,
            projects: Default::default(),
            contexts: Default::default(),
            description_norm: "buy ducks".to_owned(),
            line_index: 0,
        }
    }

    #[test]
    fn records_round_trip_even_when_fed_in_arbitrary_pieces() {
        let records = vec![
            BundleTailRecord::Blob {
                path: "todo.txt".to_owned(),
                bytes: vec![1; 200_000],
            },
            BundleTailRecord::Fingerprint {
                path: "todo.txt".to_owned(),
                task: TaskId::new(Ulid::from_u128(1)),
                fingerprint: fp(),
                updated_at_ms: 42,
            },
        ];
        let mut whole = Vec::new();
        for r in &records {
            r.encode_into(&mut whole).unwrap();
        }
        // Feed it back in small, deliberately misaligned pieces.
        let mut reader = BundleRecordReader::default();
        let mut got = Vec::new();
        for chunk in whole.chunks(97) {
            got.extend(reader.feed(chunk).unwrap());
        }
        reader.finish().unwrap();
        assert_eq!(got, records);
        let _ = DeviceId::new(Ulid::from_u128(0)); // silence an unused-import lint on some targets
    }

    #[test]
    fn a_truncated_stream_is_reported_as_truncated_not_silently_dropped() {
        let mut whole = Vec::new();
        BundleTailRecord::Blob {
            path: "todo.txt".to_owned(),
            bytes: vec![9; 10],
        }
        .encode_into(&mut whole)
        .unwrap();
        whole.truncate(whole.len() - 2); // chop off the last two bytes of the body
        let leftover = whole.len();
        let mut reader = BundleRecordReader::default();
        reader.feed(&whole).unwrap();
        assert!(matches!(
            reader.finish(),
            Err(RecordReadError::Truncated(n)) if n == leftover
        ));
    }
}
