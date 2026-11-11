//! Key and pitch columns of the `analysis` table (T3.5, ADR-022): `key`,
//! `key_conf`, `key_source`, `pitch_midi`, `pitch_conf`.
//!
//! [`upsert_key_pitch`] writes only these five columns, so kind/tempo and
//! category are left untouched; a new row has them NULL. A missing key or
//! note stores NULL, clearing any earlier value. `key` is the
//! [`crate::text::Key`] display form (`C#m`, `Ebmaj` → `D#maj`, `A` when
//! the mode is unknown), which [`crate::search::keys::parse_key`] reads
//! back. User overrides live in `overrides` and are never written here.

use rusqlite::{params, Connection, OptionalExtension};

use super::Store;
use crate::key::{KeyNote, KeySource};
use crate::search::keys::parse_key;
use crate::text::Key;
use crate::Result;

/// The key and note of one file as stored.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct KeyPitch {
    /// Key (`mode: None` for a one-shot's note or a mode-less name).
    pub key: Option<Key>,
    /// Confidence of `key` (`0..=1`).
    pub key_conf: Option<f32>,
    /// Where `key` came from.
    pub key_source: Option<KeySource>,
    /// MIDI note of a pitched one-shot (whole number).
    pub pitch_midi: Option<u8>,
    /// Confidence of `pitch_midi` (`0..=1`).
    pub pitch_conf: Option<f32>,
}

impl KeyPitch {
    /// Row values for a [`KeyNote`].
    pub fn new(kn: &KeyNote) -> Self {
        Self {
            key: kn.key.map(|k| k.key),
            key_conf: kn.key.map(|k| k.confidence),
            key_source: kn.key.map(|k| k.source),
            pitch_midi: kn.note.map(|n| n.midi),
            pitch_conf: kn.note.map(|n| n.confidence),
        }
    }
}

/// A confidence the table's CHECK accepts (finite, clamped to `0..=1`).
fn conf(v: Option<f32>) -> Option<f64> {
    v.filter(|c| c.is_finite())
        .map(|c| f64::from(c.clamp(0.0, 1.0)))
}

/// Inserts the five key/pitch columns of `file_id`, or updates only those
/// on an existing row.
///
/// # Errors
/// [`crate::Error::Sqlite`], e.g. a foreign-key violation when `file_id`
/// has no `files` row.
pub fn upsert_key_pitch(conn: &Connection, file_id: i64, v: &KeyPitch) -> Result<()> {
    let key = v.key.map(|k| {
        Key {
            root: k.root % 12,
            mode: k.mode,
        }
        .to_string()
    });
    let (key_conf, key_source) = match key {
        Some(_) => (conf(v.key_conf), v.key_source.map(KeySource::as_str)),
        None => (None, None),
    };
    let pitch = v.pitch_midi.filter(|&m| m <= 127);
    let pitch_conf = pitch.and(conf(v.pitch_conf));
    conn.prepare_cached(
        "INSERT INTO analysis (file_id, key, key_conf, key_source, pitch_midi, pitch_conf) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
         ON CONFLICT (file_id) DO UPDATE SET \
           key = excluded.key, key_conf = excluded.key_conf, key_source = excluded.key_source, \
           pitch_midi = excluded.pitch_midi, pitch_conf = excluded.pitch_conf",
    )?
    .execute(params![
        file_id,
        key,
        key_conf,
        key_source,
        pitch.map(f64::from),
        pitch_conf,
    ])?;
    Ok(())
}

/// The key/pitch columns of `file_id`, if it has an `analysis` row. An
/// unparsable `key` or a non-integral/out-of-range `pitch_midi` reads as
/// `None`.
///
/// # Errors
/// [`crate::Error::Sqlite`].
pub fn get_key_pitch(conn: &Connection, file_id: i64) -> Result<Option<KeyPitch>> {
    type Raw = (
        Option<String>,
        Option<f64>,
        Option<String>,
        Option<f64>,
        Option<f64>,
    );
    let raw: Option<Raw> = conn
        .query_row(
            "SELECT key, key_conf, key_source, pitch_midi, pitch_conf FROM analysis \
             WHERE file_id = ?1",
            [file_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?;
    let Some((key, key_conf, source, pitch, pitch_conf)) = raw else {
        return Ok(None);
    };
    let key_source = match source.as_deref() {
        Some("detected") => Some(KeySource::Detected),
        Some("filename") => Some(KeySource::Filename),
        _ => None,
    };
    let pitch_midi = pitch
        .filter(|m| m.fract() == 0.0 && (0.0..=127.0).contains(m))
        .map(|m| m as u8);
    let f = |v: Option<f64>| v.map(|x| x as f32);
    Ok(Some(KeyPitch {
        key: key.as_deref().and_then(parse_key),
        key_conf: f(key_conf),
        key_source,
        pitch_midi,
        pitch_conf: f(pitch_conf),
    }))
}

impl Store {
    /// [`upsert_key_pitch`] on this store's connection.
    ///
    /// # Errors
    /// See [`upsert_key_pitch`].
    pub fn put_key_pitch(&self, file_id: i64, v: &KeyPitch) -> Result<()> {
        upsert_key_pitch(self.conn(), file_id, v)
    }

    /// [`get_key_pitch`] on this store's connection.
    ///
    /// # Errors
    /// See [`get_key_pitch`].
    pub fn key_pitch(&self, file_id: i64) -> Result<Option<KeyPitch>> {
        get_key_pitch(self.conn(), file_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::Mode;

    fn store_with_file() -> (Store, i64) {
        let store = Store::open_in_memory().unwrap();
        store
            .conn()
            .execute_batch(
                "INSERT INTO libraries (id, root_path, added_at) VALUES (1, '/lib', 0);
                 INSERT INTO files (id, library_id, rel_path, size, mtime)
                   VALUES (7, 1, 'a.wav', 10, 0);",
            )
            .unwrap();
        (store, 7)
    }

    fn row(mode: Option<Mode>) -> KeyPitch {
        KeyPitch {
            key: Some(Key { root: 1, mode }),
            key_conf: Some(0.75),
            key_source: Some(KeySource::Detected),
            pitch_midi: Some(49),
            pitch_conf: Some(0.5),
        }
    }

    #[test]
    fn round_trip_every_mode_and_clear() {
        let (store, id) = store_with_file();
        assert_eq!(store.key_pitch(id).unwrap(), None);
        for mode in [Some(Mode::Minor), Some(Mode::Major), None] {
            store.put_key_pitch(id, &row(mode)).unwrap();
            assert_eq!(store.key_pitch(id).unwrap(), Some(row(mode)));
        }
        let stored: String = store
            .conn()
            .query_row("SELECT key FROM analysis WHERE file_id = ?1", [id], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(stored, "C#");
        store.put_key_pitch(id, &KeyPitch::default()).unwrap();
        assert_eq!(store.key_pitch(id).unwrap(), Some(KeyPitch::default()));
    }

    #[test]
    fn kind_tempo_columns_are_untouched_and_bad_values_dropped() {
        let (store, id) = store_with_file();
        store
            .conn()
            .execute(
                "INSERT INTO analysis (file_id, kind, bpm, bpm_source) \
                 VALUES (?1, 'loop', 120.0, 'detected')",
                [id],
            )
            .unwrap();
        let bad = KeyPitch {
            key_conf: Some(f32::NAN),
            pitch_midi: Some(200),
            pitch_conf: Some(2.0),
            ..row(Some(Mode::Minor))
        };
        store.put_key_pitch(id, &bad).unwrap();
        let got = store.key_pitch(id).unwrap().unwrap();
        assert_eq!(
            (got.key_conf, got.pitch_midi, got.pitch_conf),
            (None, None, None)
        );
        let (kind, bpm): (String, f64) = store
            .conn()
            .query_row(
                "SELECT kind, bpm FROM analysis WHERE file_id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((kind.as_str(), bpm), ("loop", 120.0));
        assert!(store.put_key_pitch(999, &row(None)).is_err());
    }
}
