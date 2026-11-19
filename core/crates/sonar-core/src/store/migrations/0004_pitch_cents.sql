-- Migration 0004: `analysis.pitch_cents` (T10.6, ADR-041): deviation of a
-- one-shot's measured pitch from `pitch_midi` in cents (-50..=50), shown in
-- the inspector (`E2 +32 ct`). NULL until the file is re-analysed.

ALTER TABLE analysis ADD COLUMN pitch_cents REAL CHECK (pitch_cents BETWEEN -50 AND 50);

-- `analysis.key_audio`: the audio's own key when the filename outweighed a
-- disagreeing audio candidate (`key_source = 'filename'`), so the inspector
-- can show both. Same text form as `key`.
ALTER TABLE analysis ADD COLUMN key_audio TEXT;
