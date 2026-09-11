# Hand-write a .tokens.json oracle beside each corpus file

Plan M0: "Each corpus file has a sibling `.tokens.json` with the expected token stream (write these by
hand; they are the oracle)." M1 acceptance: `just corpus` — every line round-trips byte-for-byte and its
token stream equals the oracle.

## Format
One JSON array per corpus file; one entry per line, in file order (blank lines included).
Offsets are **byte** offsets into the raw line, excluding the line ending. Spans must be contiguous and
cover `[0, len)` exactly (plan M1: "tokens must cover every byte (`Whitespace` included)").
```json
[
  { "raw": "(A) 2026-09-11 Call the plumber +house @phone due:2026-09-15",
    "spans": [
      { "kind": "Priority",     "start": 0,  "end": 3  },
      { "kind": "Whitespace",   "start": 3,  "end": 4  },
      { "kind": "CreationDate", "start": 4,  "end": 14 },
      { "kind": "Whitespace",   "start": 14, "end": 15 },
      { "kind": "Text",         "start": 15, "end": 31 },
      { "kind": "Project",      "start": 32, "end": 38 }, …
    ] },
  { "raw": "", "spans": [] }
]
```
`kind` ∈ the M1 `TokenKind` enum: `CompletionMarker CompletionDate CreationDate Priority Project Context
TagKey TagValue IdTag Url Text Whitespace`. Decisions to fix now and keep:
- `key:value` → `TagKey` covers `key:` (colon included), `TagValue` covers the value. `id:…` is one `IdTag` span.
- A URL is one `Url` span; `mail bob@example.com` is `Text` (no context).
- Multi-byte chars: `买菜` is 6 bytes, so `+家务` starts at byte 7. Compute with `printf '%s' "$line" | wc -c`.
- `x 2026-09-11 (A) task` (lenient): `(A)` after the date tokenises as `Priority` even though it is a quirk.

## Self-check script (tier 3, ≤ 40 lines, node)
For each `*.tokens.json`: parse; for each entry check `spans` sorted, contiguous, `start==0`, last `end ==
Buffer.byteLength(raw)`, kinds in the enum; also check `raw` equals the corresponding line of the `.txt`
(read as bytes, split on the file's ending, strip the BOM for `bom.txt`). Exit 1 on any mismatch.
Ref: https://nodejs.org/api/buffer.html#static-method-bufferbytelengthstring-encoding
