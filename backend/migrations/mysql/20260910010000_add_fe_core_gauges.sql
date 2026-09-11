ALTER TABLE metrics_snapshots ADD COLUMN meta_log_count BIGINT NOT NULL DEFAULT 0;
ALTER TABLE metrics_snapshots ADD COLUMN unfinished_query BIGINT NOT NULL DEFAULT 0;
ALTER TABLE metrics_snapshots ADD COLUMN safe_mode INTEGER NOT NULL DEFAULT 0;
