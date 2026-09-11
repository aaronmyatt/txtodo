# Seed corpus/ with 30+ lines covering every §2.4 edge case and the ref cases

Plan M0: "Seed `corpus/` with at least 30 hand-written lines covering every row of the design doc's
edge-case table (§2.4) plus the `ref:` cases. Each corpus file has a sibling `.tokens.json`."
The oracle files are the next task (corpus-token-oracle); this task writes only the `.txt` side.

## Layout
```
corpus/
  README.md                 theme → file → rule
  edge-cases.txt            design §2.4, one row per line, table order
  lenient.txt               each leniency exactly once
  tags.txt                  every Appendix A tag plus an unknown key and a non-tag `note:`
  refs.txt                  valid and invalid slugs (invalid → quirk invalid_ref, treated as no ref)
  structure.txt             blank-line placement, unicode, several projects/contexts on one line
  crlf.txt                  every line ends \r\n
  bom.txt                   starts with EF BB BF
  no-trailing-newline.txt   last line has no newline
  mixed-endings.txt         LF and CRLF in one file → per-line quirk
```

## Lines that must exist (write these exactly)
```
mail bob@example.com
see https://example.com/x
learn C++ +cpp
买菜 +家务 @手机
X 2026-09-11 not done
x 2026-09-11 (A) task
note: buy milk
(a) task
x 2026-09-11 2026-09-01 Renew passport +admin pri:A
(A) 2026-09-11 Call the plumber +house @phone due:2026-09-15 t:2026-09-14 rec:1w
2026-09-11 Q4 roadmap +work ref:q4-roadmap id:01J9K3H5Z7Q8X2M4N6P8R0T2V4
2026-09-11 Bad ref ref:../escape
2026-09-11 Bad ref ref:/etc/passwd
2026-09-11 Bad ref ref:.
2026-09-11 Bad ref ref:Q4-Roadmap
```
Plus enough plain lines to reach 30 task lines. Blank lines are entries too but do not count toward 30.

## Rules
- Hand-written, never generated: the corpus is the oracle for M1, not a product of it.
- No real personal data. Synthetic names only.
- Bytes matter: create `crlf.txt`, `bom.txt`, `no-trailing-newline.txt` with `printf`, then verify with
  `xxd | head` and commit. Add a `.gitattributes` line `corpus/* -text` so git never normalises them.
  Ref: https://git-scm.com/docs/gitattributes#_text
