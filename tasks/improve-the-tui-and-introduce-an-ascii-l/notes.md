# TUI logo and an ASCII logo in `txtodo --help`

## Goal
- The TUI header's mark reads as the txtodo logo (assets/brand/tx-mark.svg: a t whose crossbar
  hooks down like a check, then an x).
- `txtodo --help` opens with an ASCII version of the same logo.
- Asked by the human 2026-10-11.

## Today
- TUI: `ui/mark.rs` draws four box glyphs `┼╰╲╱`, one per stroke, lit by list progress. Side by
  side they don't read as "tx"; the x's two bars sit apart instead of crossing.
- CLI: no logo; clap's `about` only.

## Design
- TUI: keep it one header row and keep the four-stroke progress meaning (desktop's Mark does the
  same). Pick glyphs that read as t + check-hook + x in 3 cells; record the pick here.
- CLI: a small multi-line ASCII mark (plain ASCII, no colour, so pipes and `less` stay clean) via
  clap `before_long_help`: `--help` shows it, `-h` stays terse.
  Ref: https://docs.rs/clap/latest/clap/struct.Command.html#method.before_long_help

## Out
- No colour or animation in the CLI logo.

## As built
- TUI mark: `┼╲ ╳` (t with its bar, the bar's hook down like a check, a gap, the x), 4 cells as
  before. The old `┼╰╲╱` split the x into two cells that read as a V. The x is one cell now, so
  its first stroke lights as a lone bold `╲`, the second as a bold `╳`. Unit test pins the glyphs.
  Rejected: letters `tx` (no hook, nothing to light by stroke).
