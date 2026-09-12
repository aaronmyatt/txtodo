//! Task identity mode (design §4.1): **tagged** (an `id:<ULID>` tag on every task line, the only
//! mode built so far) or **sidecar** (no tags in the file; identity lives in a fingerprint index,
//! re-matched after an external edit by an assignment-problem solver). A workspace mints its mode
//! once and keeps it for its lifetime (`txtodo-daemon`'s `workspace.rs`, same idiom as
//! `device_id`/`group_id`); this crate only holds the mode-agnostic shapes both `txtodo-cli` and
//! `txtodo-daemon` need, never the matching algorithm itself (that needs a Levenshtein
//! implementation this crate does not, and must not, depend on).

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// How a workspace establishes task-line identity. Fixed for a workspace's lifetime once minted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum IdentityMode {
    /// Every task line carries an `id:<ULID>` tag.
    Tagged,
    /// No tags in the file; identity lives in a fingerprint index (`docs/questions.md` Q2).
    Sidecar,
}

/// What a task line looks like for fingerprint matching (sidecar mode only). Pure data: no
/// algorithm here, so this crate never needs a string-distance dependency.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fingerprint {
    /// `(year, month, day)`, raw — mirrors `txtodo_model::op::FieldValue::Date` since
    /// `txtodo-core`'s `Date` has no serde.
    pub creation_date: Option<(u16, u8, u8)>,
    /// `+project` names, sorted (constitution §3: no `HashSet` reachable from stored shapes).
    pub projects: BTreeSet<String>,
    /// `@context` names, sorted.
    pub contexts: BTreeSet<String>,
    /// The description, trimmed and lowercased, for edit-distance comparison.
    pub description_norm: String,
    /// 0-based index among task lines only (blanks excluded) at the time this was captured.
    pub line_index: usize,
}

/// The cost-function weights and match/no-match threshold for sidecar re-identification
/// (`docs/questions.md` Q2). `DEFAULT` is v1 and deliberately tunable — retune the constants
/// directly, no ADR needed for a weight change alone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CostWeights {
    /// Weight for a creation-date mismatch (0.0 or 1.0 term).
    pub date: f64,
    /// Weight for `1 - jaccard(projects)`.
    pub project: f64,
    /// Weight for `1 - jaccard(contexts)`.
    pub context: f64,
    /// Weight for the normalised Levenshtein distance between descriptions.
    pub description: f64,
    /// Weight for the normalised line-position delta.
    pub position: f64,
    /// A candidate match costing at or above this is rejected (becomes delete+insert instead of
    /// a match). Set just under `description`'s own weight: a fully-rewritten description with
    /// everything else identical is deliberately classified as a duplicate, not a merge — the
    /// design's own stated preference for "resurrect as duplicate" over a wrong merge.
    pub match_threshold: f64,
}

impl CostWeights {
    /// v1 weights (`docs/questions.md` Q2). `crates/txtodo-daemon/src/identity/` computes the
    /// actual terms (jaccard, normalised Levenshtein) and combines them with these.
    pub const DEFAULT: CostWeights = CostWeights {
        date: 3.0,
        project: 2.0,
        context: 1.0,
        description: 6.0,
        position: 1.5,
        match_threshold: 5.0,
    };
}

// A full description rewrite (normalised distance 1.0) costs `description` alone; the threshold
// must sit below that so a rewrite is classified as a duplicate, never force-matched. Checked at
// compile time, not just by a test, since both sides are already `const`.
const _: () = assert!(CostWeights::DEFAULT.match_threshold < CostWeights::DEFAULT.description);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_mode_round_trips_through_postcard() {
        for mode in [IdentityMode::Tagged, IdentityMode::Sidecar] {
            let bytes = postcard::to_allocvec(&mode).unwrap();
            let back: IdentityMode = postcard::from_bytes(&bytes).unwrap();
            assert_eq!(back, mode);
        }
    }
}
