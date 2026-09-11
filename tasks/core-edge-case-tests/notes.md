# Named unit test for every row of design §2.4

Plan M1 acceptance: "Every row of the design doc's edge-case table (§2.4) is a named unit test." The corpus
already holds the lines (`corpus/edge-cases.txt`); these tests state the *behaviour* the table promises,
in plain assertions a reader can match to the table row by row.

| Test name | Line | Asserts |
|---|---|---|
| `email_at_is_not_a_context` | `mail bob@example.com` | `contexts()` empty; tokens `Text WS Text` |
| `url_is_not_a_tag` | `see https://example.com/x` | `tags()` empty; second word `Url` |
| `plus_inside_word_is_not_a_project` | `learn C++ +cpp` | `projects() == ["cpp"]` |
| `unicode_project_and_context` | `买菜 +家务 @手机` | `projects() == ["家务"]`, `contexts() == ["手机"]` |
| `uppercase_x_is_not_completion` | `X 2026-09-11 not done` | `completed == false`, `creation_date == None`, description is the whole line |
| `priority_after_x_is_lenient_quirk` | `x 2026-09-11 (A) task` | strict `Err(rule="completed")`; lenient `priority == Some(A)`, `PRIORITY_AFTER_DATE` set |
| `word_with_trailing_colon_is_text` | `note: buy milk` | `tags()` empty; first token `Text` |
| `lowercase_priority_is_text` | `(a) task` | `priority == None`; description `(a) task` |
| `crlf_is_preserved` | `a\r\nb\r\n` | `parse_file` → both lines `CrLf`; `to_bytes` identical |

Keep each test ≤ 15 lines; helper `fn parse(raw) -> Task` in the test file (test helpers may be shared within
one test file; not across slices).
