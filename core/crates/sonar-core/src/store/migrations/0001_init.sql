-- Migration 0001: initial schema (docs/ARCHITECTURE.md §3).
--
-- Applied by `store::migrations` inside one transaction that also bumps
-- `PRAGMA user_version`; never put BEGIN/COMMIT in a migration file.
--
-- Conventions:
-- * Tables are STRICT (column types enforced by SQLite).
-- * Timestamps (`added_at`, `created_at`) are unix seconds.
-- * `files.mtime` is nanoseconds since the unix epoch.
-- * `files.quick_hash` is a 64-bit hash stored as INTEGER (u64 bit-cast to
--   i64); `files.content_hash` is an opaque BLOB digest.
-- * `files.status`: 'new' (discovered) -> 'meta' (basic metadata read) ->
--   'analyzed' (DSP done); 'error' (see `files.error`); 'missing' (gone from
--   disk or volume offline, row kept for user data).
-- * `analysis.*_source`: 'filename' (prior parsed from the name) or
--   'detected' (DSP); NULL when the value is NULL.
-- * Confidences are in [0, 1].
-- * User data tables (overrides, tags, file_tags, collections,
--   collection_items, favorites, hidden) are never written by analysis code.
-- * Every child row cascades on delete of its parent.

CREATE TABLE libraries (
    id          INTEGER PRIMARY KEY,
    root_path   TEXT    NOT NULL UNIQUE,
    volume_uuid TEXT,
    bookmark    BLOB,
    online      INTEGER NOT NULL DEFAULT 1 CHECK (online IN (0, 1)),
    added_at    INTEGER NOT NULL
) STRICT;

CREATE TABLE files (
    id               INTEGER PRIMARY KEY,
    library_id       INTEGER NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    rel_path         TEXT    NOT NULL,
    size             INTEGER NOT NULL CHECK (size >= 0),
    mtime            INTEGER NOT NULL,
    quick_hash       INTEGER,
    content_hash     BLOB,
    status           TEXT    NOT NULL DEFAULT 'new'
                     CHECK (status IN ('new', 'meta', 'analyzed', 'error', 'missing')),
    analysis_version INTEGER NOT NULL DEFAULT 0,
    error            TEXT,
    UNIQUE (library_id, rel_path)
) STRICT;

-- Move/rename detection (T1.2): same size + quick hash at a new path.
CREATE INDEX files_size_quick_hash ON files (size, quick_hash);
CREATE INDEX files_status ON files (status);

CREATE TABLE audio_meta (
    file_id       INTEGER PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
    duration_ms   INTEGER CHECK (duration_ms >= 0),
    sample_rate   INTEGER CHECK (sample_rate > 0),
    channels      INTEGER CHECK (channels > 0),
    bit_depth     INTEGER CHECK (bit_depth > 0),
    peak_db       REAL,
    loudness_lufs REAL
) STRICT;

CREATE TABLE analysis (
    file_id         INTEGER PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
    kind            TEXT CHECK (kind IN ('loop', 'one_shot')),
    kind_conf       REAL CHECK (kind_conf BETWEEN 0 AND 1),
    bpm             REAL CHECK (bpm > 0),
    bpm_conf        REAL CHECK (bpm_conf BETWEEN 0 AND 1),
    bpm_source      TEXT CHECK (bpm_source IN ('filename', 'detected')),
    key             TEXT,
    key_conf        REAL CHECK (key_conf BETWEEN 0 AND 1),
    key_source      TEXT CHECK (key_source IN ('filename', 'detected')),
    pitch_midi      REAL,
    pitch_conf      REAL CHECK (pitch_conf BETWEEN 0 AND 1),
    category        TEXT,
    category_conf   REAL CHECK (category_conf BETWEEN 0 AND 1),
    category_source TEXT CHECK (category_source IN ('filename', 'detected'))
) STRICT;

CREATE INDEX analysis_category ON analysis (category);
CREATE INDEX analysis_bpm ON analysis (bpm);
CREATE INDEX analysis_key ON analysis (key);

-- ---------------------------------------------------------------- user data

-- `field` names an analysis column (e.g. 'bpm', 'key', 'category'); there
-- is deliberately no CHECK on it: the Rust layer validates field names
-- (T6.8) so new overridable fields need no table rebuild.
-- `value` is ANY so numeric overrides keep their type (REAL bpm, INTEGER
-- pitch); NULL means the user explicitly cleared the value.
CREATE TABLE overrides (
    file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    field   TEXT    NOT NULL,
    value   ANY,
    PRIMARY KEY (file_id, field)
) STRICT;

CREATE TABLE tags (
    id   INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE COLLATE NOCASE
) STRICT;

CREATE TABLE file_tags (
    file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    tag_id  INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (file_id, tag_id)
) STRICT;

CREATE INDEX file_tags_tag ON file_tags (tag_id);

CREATE TABLE collections (
    id         INTEGER PRIMARY KEY,
    name       TEXT    NOT NULL,
    created_at INTEGER NOT NULL
) STRICT;

CREATE TABLE collection_items (
    collection_id INTEGER NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    file_id       INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    position      INTEGER NOT NULL,
    PRIMARY KEY (collection_id, file_id)
) STRICT;

CREATE INDEX collection_items_file ON collection_items (file_id);
CREATE INDEX collection_items_order ON collection_items (collection_id, position);

CREATE TABLE favorites (
    file_id  INTEGER PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
    added_at INTEGER NOT NULL
) STRICT;

CREATE TABLE hidden (
    file_id INTEGER PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE
) STRICT;
