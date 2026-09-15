-- Task daemon-workspace-identity-agreement stage 2: a peer's relay reachability, captured durably
-- at pairing time instead of only living transiently inside an in-flight PairingOffer
-- (crates/txtodo-sync/src/offer.rs's relay_node_id/relay_url fields, previously discarded once the
-- handshake finished). NULL for a device paired before this migration, or paired with no relay
-- configured on either side -- a peer with no relay_node_id simply never appears in the always-on
-- control channel's per-known-peer redial loop (stage 5), a documented gap, not an error.
-- https://www.sqlite.org/lang_altertable.html
ALTER TABLE devices ADD COLUMN relay_node_id BLOB;
ALTER TABLE devices ADD COLUMN relay_url TEXT;
PRAGMA user_version = 7;
