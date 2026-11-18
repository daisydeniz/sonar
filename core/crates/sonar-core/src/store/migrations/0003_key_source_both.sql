-- Migration 0003: `analysis.key_source` may be 'both' (T10.2, ADR-041:
-- the key is fused from audio and filename; 'detected' = audio,
-- 'filename' = name, 'both' = they agree).
--
-- A CHECK cannot be altered in place, so the table is rebuilt
-- (https://www.sqlite.org/lang_altertable.html#otheralter); migrations run
-- with foreign keys OFF, so dropping the old table cascades nothing.
-- Columns and other constraints are unchanged from 0001.

CREATE TABLE analysis_new (
    file_id         INTEGER PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
    kind            TEXT CHECK (kind IN ('loop', 'one_shot')),
    kind_conf       REAL CHECK (kind_conf BETWEEN 0 AND 1),
    bpm             REAL CHECK (bpm > 0),
    bpm_conf        REAL CHECK (bpm_conf BETWEEN 0 AND 1),
    bpm_source      TEXT CHECK (bpm_source IN ('filename', 'detected')),
    key             TEXT,
    key_conf        REAL CHECK (key_conf BETWEEN 0 AND 1),
    key_source      TEXT CHECK (key_source IN ('filename', 'detected', 'both')),
    pitch_midi      REAL,
    pitch_conf      REAL CHECK (pitch_conf BETWEEN 0 AND 1),
    category        TEXT,
    category_conf   REAL CHECK (category_conf BETWEEN 0 AND 1),
    category_source TEXT CHECK (category_source IN ('filename', 'detected'))
) STRICT;

INSERT INTO analysis_new
    (file_id, kind, kind_conf, bpm, bpm_conf, bpm_source, key, key_conf, key_source,
     pitch_midi, pitch_conf, category, category_conf, category_source)
SELECT file_id, kind, kind_conf, bpm, bpm_conf, bpm_source, key, key_conf, key_source,
       pitch_midi, pitch_conf, category, category_conf, category_source
FROM analysis;

DROP TABLE analysis;
ALTER TABLE analysis_new RENAME TO analysis;

CREATE INDEX analysis_category ON analysis (category);
CREATE INDEX analysis_bpm ON analysis (bpm);
CREATE INDEX analysis_key ON analysis (key);
