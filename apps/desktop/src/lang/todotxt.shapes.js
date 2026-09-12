// GENERATED FILE — do not hand-edit.
// Produced by apps/desktop/scripts/abnf-to-lezer.mjs from specs/todotxt.abnf.
// Regenerate: node apps/desktop/scripts/abnf-to-lezer.mjs

export const COMPLETION_MARKER = "x";
export const PRIORITY_RANGE = [65,90];
export const DATE_SHAPE = [{"digits":4},{"literal":"-"},{"digits":2},{"literal":"-"},{"digits":2}];
export const ULID_PREFIX = "id:";
export const ULID_LENGTH = 26;
export const ULID_RANGES = [[48,57],[65,72],[74,75],[77,78],[80,84],[86,90]];
export const KEY_RANGES = [[33,57],[59,126],[128,1114111]]; // excludes ':' and space already (see specs/todotxt.abnf's key rule)
export const NONSP_RANGES = [[33,126],[128,1114111]];
