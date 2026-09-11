# Sidecar identity mode with fingerprint assignment via the Hungarian algorithm (plan M10)

Goal: no `id:` tags in the file; IDs live in `.txtodo/index` keyed by a fingerprint, re-identified after external edits.

Design: txtodo-design.md §4.1 — assignment cost = creation-date equality + project/context overlap + normalised Levenshtein + position distance, solved with the Hungarian algorithm; below-threshold matches become delete+insert.

Plan: txtodo-implementation-plan.md M10 — mini-plan deferred until started.
