-- Sync heads (plan M4, tasks/sync-protocol-frames). A head is "how many of device X's ops we hold"
-- and an op's origin_seq is its rank in its own device's HLC order — both derived from rows that
-- already exist, never stored, so the log stays append-only (no backfill UPDATE). The sync
-- protocol commits a device's ops only as contiguous runs from head + 1 (Ack carries committed
-- runs, advance() refuses a gap), so COUNT(*) per device is exactly the head.
-- https://www.sqlite.org/lang_createindex.html · https://www.sqlite.org/pragma.html#pragma_user_version
CREATE INDEX ops_device_hlc ON ops(device, hlc_wall, hlc_counter);
PRAGMA user_version = 2;
