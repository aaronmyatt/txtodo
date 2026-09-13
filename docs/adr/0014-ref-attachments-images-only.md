# 0014 — Only images inside a ref: directory sync as attachments

- Status: accepted
- Date: 2026-09-13
- Deciders: project owner (docs/questions.md Q1)

## Context
Design plan §3.2.11 syncs only `todo.txt`, `notes.md` within a workspace or `ref:`
directory. Other non-managed files sitting alongside them (attachments) were left undecided —
question was whether they sync at all.

## Decision
We will sync image files inside a `ref:` directory as attachments, on the assumption they are
referenced from `notes.md` write-ups. No other non-managed file type syncs.

## Consequences
- Good: covers the concrete use case (images embedded in markdown notes) without opening sync
  scope to arbitrary user files.
- Bad: needs a file-type allowlist (extension or content-sniffed) and a payload-size policy for
  image attachments that didn't exist before.
- Neutral / follow-ups: M5 sync scope and M8 relay payloads (docs/questions.md Q1 blocks) now
  include an image-attachment path; PDFs, audio, and other non-image files stay out of scope
  unless raised again.

## Alternatives considered
- Sync no non-managed files (the prior default): simplest, but breaks embedded-image write-ups.
- Sync all non-managed files: unbounded payload/privacy surface for no stated use case.
