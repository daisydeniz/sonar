//! Basic metadata rows (`audio_meta` table, one per file; T1.4).
//!
//! Absent values (`bit_depth` of lossy files, `peak_db`/`loudness_lufs` of
//! silence) are NULL. `measured_s` and `partial` of [`BasicMeta`] are not
//! stored: partial decodes are recorded on the `files` row by the pipeline
//! (T1.5). The free functions take a [`Connection`] so the pipeline can
//! call them inside its own write transaction (a `Transaction` derefs to
//! one); [`Store`] wraps them for single calls.

use rusqlite::{params, Connection, OptionalExtension};

use super::Store;
use crate::meta::BasicMeta;
use crate::Result;

/// An `audio_meta` row.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredMeta {
    /// Playable length (ms).
    pub duration_ms: u64,
    /// Sample rate (Hz).
    pub sample_rate: u32,
    /// Channel count.
    pub channels: u16,
    /// Bits per sample; `None` for lossy files.
    pub bit_depth: Option<u32>,
    /// Sample peak (dBFS); `None` for digital silence.
    pub peak_dbfs: Option<f64>,
    /// Integrated loudness (LUFS); `None` below the gate.
    pub loudness_lufs: Option<f64>,
}

impl From<&BasicMeta> for StoredMeta {
    fn from(m: &BasicMeta) -> Self {
        Self {
            duration_ms: m.duration_ms,
            sample_rate: m.sample_rate,
            channels: m.channels,
            bit_depth: m.bit_depth,
            peak_dbfs: m.peak_dbfs,
            loudness_lufs: m.loudness_lufs,
        }
    }
}

/// Inserts or replaces the row of `file_id`. Non-finite floats are stored
/// as NULL (they never come out of [`crate::meta`]; belt and braces).
///
/// # Errors
/// [`crate::Error::Sqlite`], e.g. a foreign-key violation when `file_id`
/// has no `files` row.
pub fn upsert(conn: &Connection, file_id: i64, meta: &BasicMeta) -> Result<()> {
    let finite = |v: Option<f64>| v.filter(|x| x.is_finite());
    let duration = i64::try_from(meta.duration_ms).unwrap_or(i64::MAX);
    conn.execute(
        "INSERT INTO audio_meta \
           (file_id, duration_ms, sample_rate, channels, bit_depth, peak_db, loudness_lufs) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
         ON CONFLICT (file_id) DO UPDATE SET \
           duration_ms = excluded.duration_ms, sample_rate = excluded.sample_rate, \
           channels = excluded.channels, bit_depth = excluded.bit_depth, \
           peak_db = excluded.peak_db, loudness_lufs = excluded.loudness_lufs",
        params![
            file_id,
            duration,
            meta.sample_rate,
            meta.channels,
            meta.bit_depth,
            finite(meta.peak_dbfs),
            finite(meta.loudness_lufs),
        ],
    )?;
    Ok(())
}

/// The row of `file_id`, if any.
///
/// # Errors
/// [`crate::Error::Sqlite`], including a row whose integers do not fit
/// the Rust types (cannot be written by [`upsert`]).
pub fn get(conn: &Connection, file_id: i64) -> Result<Option<StoredMeta>> {
    let row = conn
        .query_row(
            "SELECT duration_ms, sample_rate, channels, bit_depth, peak_db, loudness_lufs \
             FROM audio_meta WHERE file_id = ?1",
            [file_id],
            |r| {
                let ms: i64 = r.get(0)?;
                Ok(StoredMeta {
                    duration_ms: u64::try_from(ms)
                        .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, ms))?,
                    sample_rate: r.get(1)?,
                    channels: r.get(2)?,
                    bit_depth: r.get(3)?,
                    peak_dbfs: r.get(4)?,
                    loudness_lufs: r.get(5)?,
                })
            },
        )
        .optional()?;
    Ok(row)
}

impl Store {
    /// [`upsert`] on this store's connection.
    ///
    /// # Errors
    /// See [`upsert`].
    pub fn put_audio_meta(&self, file_id: i64, meta: &BasicMeta) -> Result<()> {
        upsert(self.conn(), file_id, meta)
    }

    /// [`get`] on this store's connection.
    ///
    /// # Errors
    /// See [`get`].
    pub fn audio_meta(&self, file_id: i64) -> Result<Option<StoredMeta>> {
        get(self.conn(), file_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn sample() -> BasicMeta {
        BasicMeta {
            duration_ms: 1_500,
            num_frames: 66_150,
            sample_rate: 44_100,
            channels: 2,
            bit_depth: Some(24),
            peak_dbfs: Some(-6.0206),
            loudness_lufs: Some(-9.03),
            measured_s: 1.5,
            partial: false,
        }
    }

    #[test]
    fn round_trip_and_replace() {
        let (store, id) = store_with_file();
        assert_eq!(store.audio_meta(id).unwrap(), None);
        let m = sample();
        store.put_audio_meta(id, &m).unwrap();
        assert_eq!(store.audio_meta(id).unwrap(), Some(StoredMeta::from(&m)));

        // Lossy + silent: NULLs, and the upsert replaces every column.
        let silent = BasicMeta {
            bit_depth: None,
            peak_dbfs: None,
            loudness_lufs: Some(f64::NAN),
            ..sample()
        };
        store.put_audio_meta(id, &silent).unwrap();
        let got = store.audio_meta(id).unwrap().unwrap();
        assert_eq!(
            (got.bit_depth, got.peak_dbfs, got.loudness_lufs),
            (None, None, None)
        );
        let nulls: i64 = store
            .conn()
            .query_row(
                "SELECT count(*) FROM audio_meta WHERE bit_depth IS NULL \
                 AND peak_db IS NULL AND loudness_lufs IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(nulls, 1);
        let rows: i64 = store
            .conn()
            .query_row("SELECT count(*) FROM audio_meta", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 1);
    }

    #[test]
    fn unknown_file_is_a_foreign_key_error_and_rows_cascade() {
        let (store, id) = store_with_file();
        assert!(store.put_audio_meta(999, &sample()).is_err());
        store.put_audio_meta(id, &sample()).unwrap();
        store
            .conn()
            .execute("DELETE FROM files WHERE id = ?1", [id])
            .unwrap();
        assert_eq!(store.audio_meta(id).unwrap(), None);
    }

    #[test]
    fn zero_length_file_is_storable() {
        let (store, id) = store_with_file();
        let empty = BasicMeta {
            duration_ms: 0,
            num_frames: 0,
            peak_dbfs: None,
            loudness_lufs: None,
            measured_s: 0.0,
            ..sample()
        };
        store.put_audio_meta(id, &empty).unwrap();
        assert_eq!(store.audio_meta(id).unwrap().unwrap().duration_ms, 0);
    }
}
