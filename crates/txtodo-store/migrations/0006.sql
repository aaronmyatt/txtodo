-- Known sync-group peers (plan M4 tasks/sync-device-remove): one row per device this workspace has
-- paired with, holding the long-term X25519 static public key a rotation wraps a new group key to
-- (crates/txtodo-sync/src/device_static.rs's `DeviceStaticPublic`, registered once at pairing so an
-- offline device still finds its grant when it returns) and enough bookkeeping for
-- `txtodo device list`/`remove` and `txtodo doctor`'s per-peer clock-skew line
-- (tasks/model-hlc-skew-guard). Removal follows the crate's upsert idiom (`tokens.rs`'s
-- `revoke_token`): `removed_at` NULL means an active peer, so there is still no bare `UPDATE`
-- statement of this crate's own — a removed row is kept, never deleted, the same tombstone
-- discipline as `fingerprints`/`tokens`. `key_epoch` is the newest epoch this device is known to
-- hold (set at registration, advanced when a rotation mints it a grant) — it is this crate's local
-- bookkeeping of what we believe we handed the device, not the group's current epoch, which lives
-- in the keystore (`crates/txtodo-sync/src/keystore.rs`'s `KeyId::Group(epoch)`).
-- `last_known_wall` is the peer's own clock reading last observed by this device (e.g. a pairing
-- offer's `issued_at_ms`); NULL until one is seen, so `txtodo doctor` can say "no clock sample yet"
-- rather than fabricate a skew reading.
-- https://www.sqlite.org/lang_createtable.html · https://www.sqlite.org/lang_upsert.html
CREATE TABLE devices (
    device            BLOB    PRIMARY KEY,
    name              TEXT    NOT NULL,
    static_public     BLOB    NOT NULL,
    paired_at         INTEGER NOT NULL,
    last_seen         INTEGER,
    last_known_wall   INTEGER,
    key_epoch         INTEGER NOT NULL DEFAULT 0,
    removed_at        INTEGER
);
PRAGMA user_version = 6;
