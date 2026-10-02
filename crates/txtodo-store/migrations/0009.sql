-- ADR 0039: an op's origin_seq (its number in its device's run, what sync heads and wants name) is
-- stored once, when the row is written. It used to be the row's rank in its device's HLC order,
-- which moves when a later op of that device sorts before ops already sent. Existing rows take
-- today's rank, the number this device and its peers last used; from here on it never changes.
-- Window functions: https://www.sqlite.org/windowfunctions.html
ALTER TABLE ops ADD COLUMN origin_seq INTEGER;
CREATE TEMP TABLE origin_ranks AS
  SELECT seq, ROW_NUMBER() OVER (PARTITION BY device ORDER BY hlc_wall, hlc_counter, seq) AS n
  FROM ops;
CREATE INDEX temp.origin_ranks_seq ON origin_ranks (seq);
UPDATE ops SET origin_seq = (SELECT n FROM origin_ranks WHERE origin_ranks.seq = ops.seq);
DROP TABLE temp.origin_ranks;
CREATE UNIQUE INDEX ops_device_origin_seq ON ops (device, origin_seq);
PRAGMA user_version = 9;
