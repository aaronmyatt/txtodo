# Desktop UI revamp mockups

The design record for `tasks/desktop-ui-revamp`. `c2-prompt.html` (with `c2/` and `shared/`) is
the spec; `a-paper.html`, `b-tile.html` and `c-prompt.html` are history.

- View: `python3 -m http.server 8765` here, then open `/c2-prompt.html`. Tailwind and DaisyUI load
  from a CDN, so viewing needs the internet.
- Checks: `?selftest` in the page, or `node shared/selftest.mjs`.
- The app keeps CodeMirror 6: take the look and behaviour from here, never the editor code.
