// Synthetic sample workspace for every mockup (no real user data). UMD-lite like todotxt.js so
// selftest.mjs can import it. Shapes mirror what the real app gets from the daemon:
//   refs      → FileInfoDto.progress per `ref:` slug (lineInfo.ts resolveRefIndicator)
//   details   → the `<slug>/todo.txt` sub-list or `<slug>/notes.md` a `ref:` points at
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.TxData = api;
})(typeof self !== "undefined" ? self : this, function () {
  const DOC = [
    "(A) 2026-09-14 plan the Q4 roadmap +work @desk due:2026-09-26 ref:q4-roadmap",
    "(A) 2026-09-16 book flights for the offsite +work @phone due:2026-09-22",
    "(B) 2026-09-15 draft the release notes +txtodo @desk ref:release-notes",
    "(B) 2026-09-12 reply to Mara about the design review +work @email",
    "(B) 2026-09-17 buy milk and oat bars +home @errand",
    "2026-09-18 read the SQLite WAL chapter +learning @couch t:2026-09-21",
    "(C) 2026-09-10 sort the shelf photos +home @weekend",
    "2026-09-19 renew the passport +admin @online due:2026-10-15 rec:1y",
    // The one deliberately long line: shows the advisory wavy underline past 100 chars.
    "(C) 2026-09-08 write down every idea for the side project so it stops rattling, rank them by effort +ideas",
    "x 2026-09-18 2026-09-11 finish the onboarding doc +work @desk",
    "x 2026-09-17 2026-09-13 pay the electricity bill +home @online",
    "x 2026-09-16 2026-09-16 water the plants +home rec:1w",
    "", // trailing blank line = the "Add a line…" slot (design §2.6: blanks are entries)
  ].join("\n");

  const REFS = {
    "q4-roadmap": { kind: "progress", done: 1, total: 3 },
    "release-notes": { kind: "notes" },
  };

  const DETAILS = {
    "q4-roadmap": {
      list: [
        "(A) 2026-09-14 draft the outline +work @desk",
        "(B) 2026-09-15 collect input from the team +work @email",
        "x 2026-09-17 2026-09-14 list last quarter's misses +work",
        "",
      ].join("\n"),
    },
    "release-notes": {
      notes: [
        "# Release notes",
        "",
        "- CLI: `txtodo lint` reports lines over the 100-char hint",
        "- TUI: trailing `[100+]` marker past the hint",
        "- Desktop: wavy underline past the hint",
        "",
        "Still to decide: ship the new brand mark in this release or the next.",
        "",
      ].join("\n"),
    },
  };

  const WORKSPACES = [
    { name: "notes", path: "~/notes", open: 9 },
    { name: "txtodo", path: "~/Development/txtodo", open: 7 },
    { name: "home", path: "~/home-list", open: 5 },
  ];

  // The other workspaces, for the Universal view and the workspace switcher (notes is DOC above).
  const WS_DOCS = {
    txtodo: [
      "(A) 2026-09-17 ship the desktop rebrand behind a flag +desktop @desk due:2026-09-25 ref:desktop-rebrand",
      "(A) 2026-09-18 fix the flaky daemon autostart test on CI +daemon @desk due:2026-09-18",
      "(B) 2026-09-15 write the ADR for capability tokens +sync @desk",
      "(B) 2026-09-16 add txtodo lint to the pre-commit hook +cli @desk",
      "(B) 2026-09-12 review Mara's wasm-core PR +desktop @email due:2026-09-22",
      "(C) 2026-09-10 tidy the design-mockups folder +desktop @weekend",
      "2026-09-19 profile the 10k-line first paint +desktop @desk t:2026-09-23",
      "x 2026-09-18 2026-09-14 close out root todo item 9 +cli @desk",
      "x 2026-09-17 2026-09-15 line-length hint in the TUI +tui @desk",
      "",
    ].join("\n"),
    home: [
      "(B) 2026-09-17 book the boiler service +house @phone due:2026-09-24",
      "(B) 2026-09-14 renew the car insurance +car @online due:2026-09-28",
      "(C) 2026-09-11 patch the bathroom grout +house @weekend",
      "2026-09-18 order oat milk +groceries @errand",
      "2026-09-19 call gran +family @phone due:2026-09-19",
      "x 2026-09-16 2026-09-12 return the library books +errands @errand",
      "",
    ].join("\n"),
  };
  const WS_REFS = {
    notes: REFS,
    txtodo: { "desktop-rebrand": { kind: "progress", done: 2, total: 5 } },
    home: {},
  };
  DETAILS["desktop-rebrand"] = {
    list: [
      "(A) 2026-09-17 finish the c-prompt mockups +desktop @desk",
      "(A) 2026-09-18 port the live mark to Svelte +desktop @desk",
      "(B) 2026-09-18 settings page: theme, workspaces, tokens +desktop @desk",
      "x 2026-09-18 2026-09-17 pick a direction +desktop",
      "x 2026-09-19 2026-09-17 export icon sources +desktop",
      "",
    ].join("\n"),
  };

  return { DOC, REFS, DETAILS, WORKSPACES, WS_DOCS, WS_REFS, TODAY: "2026-09-19" };
});
