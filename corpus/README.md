# corpus — real and synthetic todo.txt lines, hand-written, byte-exact

Every file here is an oracle for `txtodo-core` (plan M1): each line must round-trip byte-for-byte and
tokenise exactly as its sibling `.tokens.json` says. Hand-written, never generated. Synthetic data only.
`.gitattributes` marks `corpus/*` as `-text` so git never touches the bytes.

| File | Theme | Rule exercised |
|---|---|---|
| `edge-cases.txt` | design §2.4 rows, table order | `@`/`+` need a leading space; URL before tag; Unicode NONSP; lowercase `x`; uppercase priority; `note:` is text |
| `lenient.txt` | each leniency once | quirks `no_completion_date`, `priority_after_x`, `priority_after_date`, `tabs`, `trailing_ws`; leading `+`/`@` |
| `tags.txt` | Appendix A tags | `id` `pri` `due` `t` `rec` (both forms) `h`; unknown keys; `a:b:c`; URL with `@` |
| `refs.txt` | ref slugs | valid slugs; `.` `..` `/` uppercase leading-dash 65-char → quirk `invalid_ref` |
| `structure.txt` | line placement | leading/inner/trailing blank lines are entries; several projects/contexts; emoji |
| `crlf.txt` | endings | every line ends `\r\n`, including a blank one |
| `bom.txt` | BOM | starts `EF BB BF` |
| `no-trailing-newline.txt` | EOF | last line has no newline |
| `mixed-endings.txt` | endings | LF and CRLF in one file → per-line quirk |

Check the bytes, not the rendering: `xxd corpus/crlf.txt | head`.
