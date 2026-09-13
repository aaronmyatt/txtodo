# Self-hosting the relay

The relay (`relay/`, plan M8, design §4.5) is one carrier among several the sync engine tries —
LAN, direct QUIC hole-punching, the relay, Bluetooth LE, a file carrier, sneakernet (design
§4.5). **It is optional**: LAN alone is a complete system, and this project does not run a
public relay — self-hosting is the only option (docs/questions.md Q5's default). Run it only if
you want sync to keep working when your devices can't reach each other directly (e.g. a phone
asleep on cellular, or two networks that hole-punching can't traverse).

## What it stores, exactly

> The relay stores opaque, client-encrypted blobs keyed by device group and device, and forwards
> wake-up routing (APNs/FCM). It performs no encryption and cannot read op payloads or
> distinguish op types. It sees ciphertext and routing metadata only (design §4.6).

That sentence is the whole contract (design Appendix B: "It stores blobs it can't read and
forwards pushes. That is all it will ever do."). The relay crate has zero dependencies on this
workspace's own model/crypto crates — `relay/tests/no_txtodo_deps.rs` asserts this by reading the
crate's own manifest, so it is structurally incapable of importing the code that would let it
read what it stores, not merely configured not to.

Push forwarding is forward-only: M8 ships a logging no-op (`relay::push::NoopPush`) and M9 will
swap in a real APNs/FCM client behind the same `Push` trait, but even then the relay only routes
a wake-up — it never holds a platform push token or payload of its own; those live on-device.

## Build

```bash
cargo build --release -p relay
# binary at target/release/relay
```

## Run

```bash
relay --data-dir /var/lib/txtodo-relay --listen 0.0.0.0:8787
```

`--data-dir` is the only required flag — the relay refuses to start without one rather than
guess a location. Every flag below also has an environment-variable form; a flag on the command
line always wins over its environment variable.

| Flag | Env var | Default | Meaning |
|---|---|---|---|
| `--data-dir <PATH>` | `RELAY_DATA_DIR` | *(none — required)* | Directory holding the relay's SQLite store (`relay.db`). |
| `--listen <ADDR:PORT>` | `RELAY_LISTEN` | `127.0.0.1:8787` | Address the HTTP surface binds to. |
| `--retention-days <N>` | `RELAY_RETENTION_DAYS` | `30` *(example — see below)* | Days a blob is kept before the retention sweep removes it. |
| `--max-blob-bytes <N>` | `RELAY_MAX_BLOB_BYTES` | `262144` *(example — see below)* | Largest ciphertext blob accepted per write. |
| `--help` | | | Print the flag list and exit. |

This table is pinned against the binary's actual `--help` output by
`relay/tests/help_matches_docs.rs`: a renamed or removed flag fails that test, so this doc and
the binary cannot silently drift apart.

**The retention and max-blob-bytes numbers above are examples, not a spec.** The real defaults
live in `relay/src/bounds.rs` (`MAX_RETENTION_DAYS`, `MAX_BLOB_SIZE`), not in this document — if
they ever move, this table's *flag names* still have to match (the drift test only pins names),
but its *default values* can go stale. Check `relay --help` or `bounds.rs` for what a given build
actually defaults to. The store also enforces `MAX_BLOBS_PER_DEVICE` (oldest evicted first) and
`MAX_WAKEUP_QUEUE` (one wake-up enqueued per write, drained on delivery) — neither is a flag yet;
both live in the same file.

## Running as a service

An illustrative systemd unit is at [`deploy/systemd/relay.service`](../deploy/systemd/relay.service)
— user-scoped, `Restart=on-failure`, flags supplied through an `EnvironmentFile` (so
`RELAY_DATA_DIR` etc. never need to be hand-copied into the unit file itself). Treat it as a
starting point, not a finished production unit: adjust the user, paths and any distro-specific
hardening (`ProtectSystem=`, `NoNewPrivileges=`, ...) for your own host.

## TLS and the reverse-proxy trust boundary

The relay binds a plain HTTP listener — it does not terminate TLS itself. Put it behind a
TLS-terminating reverse proxy (Caddy, nginx, or similar) for anything reachable over the public
internet. The trust boundary this creates is simple to state: **the proxy sees only ciphertext**
— every blob the relay stores (and therefore every blob the proxy passes through) is already
client-encrypted (design §4.6), so terminating TLS in front of the relay protects the connection
metadata (source IPs, timing) without ever handing the proxy — or the relay itself — anything
that reads as a task. A minimal Caddy example:

```caddyfile
relay.example.com {
    reverse_proxy 127.0.0.1:8787
}
```

## The dumb-relay substitution

Design §4.5 is explicit that the reference relay is only one option: "any S3 or WebDAV endpoint
also works as a dumb relay." That works because the relay's job is minimal enough that a plain
object store can do it — write a blob to a key, list keys, read a blob back, byte-for-byte. If
you'd rather not run the `relay/` binary at all, you can roll your own on top of S3/WebDAV using
this layout:

- One object per stored blob, key `<group_id>/<device_id>/<stored_at_ms>-<sequence>`.
- The object body is the ciphertext blob, written exactly as received — no wrapping, no
  encoding, no metadata sidecar; the object store's own timestamps and content-length already
  cover "envelope length" and "stored-at".
- `list` = list keys under a `<group_id>/` prefix, one per device seen.
- `get` = read every object under `<group_id>/<device_id>/`, oldest key first.
- There is no `wake` endpoint to replicate at the storage layer: push forwarding is the reference
  binary's own job (`relay::push::Push`), not something an object store can do — an S3/WebDAV
  substitution gives you storage only, and a client polling that storage in place of a real
  wake-up.

## HTTP surface reference

No authentication: every blob is already ciphertext, so there is nothing behind a login worth
protecting (design §4.6) — anyone who can reach the relay can write and read blobs for any group
and device id they choose, which is why the group/device ids themselves should be treated as
capabilities, not usernames. A write is rate-limited per group (`bounds::MAX_REQUESTS_PER_GROUP_PER_WINDOW`
requests per minute) so one busy or misbehaving group can't crowd out the rest.

| Method | Path | Effect |
|---|---|---|
| `PUT` | `/v1/groups/{group}/devices/{device}/blobs` | Stores the request body as one blob; enqueues exactly one wake-up for `{device}` as part of the same request. |
| `GET` | `/v1/groups/{group}/devices/{device}/blobs` | Returns every blob stored for `{group}`/`{device}`, oldest first, as JSON `[{"stored_at_ms": ..., "blob": [...]}]`. |
| `GET` | `/v1/groups/{group}/devices` | Returns the device ids with at least one stored blob under `{group}`, as a JSON array of strings. |

That is the entire surface — put/get/list, with wake as a side effect of put, nothing else
(design §4.6, tasks/relay-reference/notes.md).
