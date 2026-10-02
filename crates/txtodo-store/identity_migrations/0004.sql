-- ADR 0029, second amendment (2026-10-02): own carries across one shared device. Each row is one
-- device a direct own peer (`voucher`) named as its own; a voucher's newest list replaces its rows.
-- WITHOUT ROWID: the key is the whole row. https://www.sqlite.org/withoutrowid.html
CREATE TABLE own_vouches (
  voucher BLOB NOT NULL,
  device BLOB NOT NULL,
  PRIMARY KEY (voucher, device)
) WITHOUT ROWID;
PRAGMA user_version = 4;
