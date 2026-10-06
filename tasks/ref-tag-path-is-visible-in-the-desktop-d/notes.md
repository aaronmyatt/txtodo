Removing it immediately severs the link and creates a new ref:path/

## As built (2026-10-06, agent)
- `decorations.ts::refTagsHidden` hides `ref:` words on the DetailView parent line (zero-width
  replace, atomic ranges so Backspace beside it takes it whole rather than a character).
- `keepRefTag(original, edited)` on save: an intact `ref:` stays where it is; a dropped or mangled
  one is replaced by the original at the end, and any other `ref:` word is dropped. So the panel
  never changes its own link. The TUI does the same (`state_tasks::with_ref`).
- Known gap: renaming a ref has no surface in the desktop app now (it never had a deliberate one;
  typing over the tag was the accident this fixes).
