-- Migration 0002: per-stage pipeline progress (T1.5, ADR-013).
--
-- One row per (file, stage) once the stage has run. A file is pending for a
-- stage when it has no row or `version` is below the stage's current version
-- (`jobs::Stage::version`), so bumping an algorithm re-runs only that stage.
-- `outcome`: 'ok', 'partial' (damaged/truncated input, see `message`) or
-- 'error' (stage failed; not retried until the content changes or the
-- version is bumped). `done_at` is unix seconds. Rows are written in the
-- same transaction as the stage's results, and cascade with the file.

CREATE TABLE file_stages (
    file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    stage   TEXT    NOT NULL CHECK (stage IN ('meta', 'dsp', 'embed')),
    version INTEGER NOT NULL,
    outcome TEXT    NOT NULL CHECK (outcome IN ('ok', 'partial', 'error')),
    message TEXT,
    done_at INTEGER NOT NULL,
    PRIMARY KEY (file_id, stage)
) STRICT;
