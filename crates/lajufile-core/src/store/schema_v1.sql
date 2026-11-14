-- LajuFile schema v1. Applied inside a Rust transaction by Store::migrate.
CREATE TABLE downloads (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  url           TEXT NOT NULL,
  final_url     TEXT,
  file_name     TEXT NOT NULL,
  save_path     TEXT NOT NULL,
  temp_path     TEXT NOT NULL,
  size          INTEGER,
  downloaded    INTEGER NOT NULL DEFAULT 0,
  status        TEXT NOT NULL,
  category      TEXT NOT NULL,
  connections   INTEGER NOT NULL,
  ranged        INTEGER NOT NULL DEFAULT 0,
  etag          TEXT,
  last_modified TEXT,
  cookie_enc    BLOB,
  referrer      TEXT,
  user_agent    TEXT,
  error         TEXT,
  queue_order   INTEGER NOT NULL,
  created_at    INTEGER NOT NULL,
  completed_at  INTEGER
);

CREATE TABLE segments (
  download_id INTEGER NOT NULL REFERENCES downloads(id) ON DELETE CASCADE,
  idx         INTEGER NOT NULL,
  start       INTEGER NOT NULL,
  end         INTEGER NOT NULL,
  done        INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (download_id, idx)
);

CREATE INDEX idx_downloads_status ON downloads(status, queue_order);
