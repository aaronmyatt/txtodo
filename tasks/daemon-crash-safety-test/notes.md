# Crash-safety test: kill -9 mid-write leaves old or new projection and a consistent op log

Plan M3 acceptance, last bullet. Two guarantees, two mechanisms: the *file* is atomic because of
temp + rename (POSIX rename is atomic on one filesystem, https://pubs.opengroup.org/onlinepubs/9699919799/functions/rename.html);
the *store* is atomic because SQLite WAL commits are (https://www.sqlite.org/atomiccommit.html).

## The ordering question
The actor appends ops and the new projection to SQLite, then renames the file. A kill between
those steps leaves the store ahead of the file. On restart the actor compares the projection row's
hash with the file's hash; a mismatch means "our write did not land" and it re-materialises from
the store and writes — one write, ending in the new state. The test's third assertion pins this.
The alternative order (file first) would leave a file the store has never heard of, which then
reconciles as an External edit and doubles the ops — wrong. Document the chosen order in the daemon
CLAUDE.md invariants.

## Making the window hittable
A "large" edit (10 k lines, so the write takes long enough) plus a random kill delay from a
seeded `StdRng` (stack.md fakes). Twenty iterations at ~200 ms each keeps the suite fast. The seed
is printed on failure so a run is reproducible. `Child::kill()` is SIGKILL on unix.

## Windows
`cfg(unix)` only; Windows lacks SIGKILL semantics for this test and `rename` over an open file
behaves differently. Recorded here, revisited at M10 with the Windows service work.
