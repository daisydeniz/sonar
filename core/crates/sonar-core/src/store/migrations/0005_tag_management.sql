-- Migration 0005: tag normalisation and management (T12.2, ADR-043).
--
-- * Tag names are normalised: trimmed, inner whitespace collapsed to one
--   space (ASCII space/tab/CR/LF here; the Rust write path also handles
--   Unicode whitespace).
-- * Names stay unique case-insensitively (`COLLATE NOCASE`; SQLite's NOCASE
--   folds ASCII only, so "Ä" and "ä" are different tags: acceptable).
--   Tags that collide after normalisation are merged into the most used
--   spelling (tie: the oldest, lowest id); their file links move over.
-- * `tag_ignored_pairs` stores "do not suggest these two as similar"
--   decisions by lower-cased name (a < b), so they survive renames of other
--   tags and are independent of tag ids.

CREATE TEMP TABLE tag_norm AS
SELECT t.id AS id,
       replace(replace(replace(t.name, char(9), ' '), char(10), ' '), char(13), ' ') AS norm,
       (SELECT COUNT(*) FROM file_tags ft WHERE ft.tag_id = t.id) AS cnt
FROM tags t;

-- Collapse runs of spaces: each pass halves a run, 8 passes cover 256.
UPDATE tag_norm SET norm = replace(norm, '  ', ' ');
UPDATE tag_norm SET norm = replace(norm, '  ', ' ');
UPDATE tag_norm SET norm = replace(norm, '  ', ' ');
UPDATE tag_norm SET norm = replace(norm, '  ', ' ');
UPDATE tag_norm SET norm = replace(norm, '  ', ' ');
UPDATE tag_norm SET norm = replace(norm, '  ', ' ');
UPDATE tag_norm SET norm = replace(norm, '  ', ' ');
UPDATE tag_norm SET norm = replace(norm, '  ', ' ');
UPDATE tag_norm SET norm = trim(norm, ' ');

-- win = the tag each one folds into (most used, then oldest, per
-- case-insensitive normalised name).
CREATE TEMP TABLE tag_map AS
SELECT id, norm,
       FIRST_VALUE(id) OVER (PARTITION BY lower(norm) ORDER BY cnt DESC, id ASC) AS win
FROM tag_norm
WHERE norm <> '';

-- Move the links of the losers to the winner, then drop the losers' links
-- (foreign keys are off during migrations, so nothing cascades).
INSERT OR IGNORE INTO file_tags (file_id, tag_id)
SELECT ft.file_id, m.win
FROM file_tags ft JOIN tag_map m ON m.id = ft.tag_id
WHERE m.id <> m.win;

DELETE FROM file_tags
WHERE tag_id IN (SELECT id FROM tag_map WHERE id <> win)
   OR tag_id NOT IN (SELECT id FROM tag_map);

CREATE TABLE tags_new (
    id   INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE COLLATE NOCASE
) STRICT;

INSERT INTO tags_new (id, name) SELECT id, norm FROM tag_map WHERE id = win;

DROP TABLE tags;
ALTER TABLE tags_new RENAME TO tags;

DROP TABLE tag_map;
DROP TABLE tag_norm;

CREATE TABLE tag_ignored_pairs (
    a TEXT NOT NULL,
    b TEXT NOT NULL,
    PRIMARY KEY (a, b),
    CHECK (a < b)
) STRICT;
