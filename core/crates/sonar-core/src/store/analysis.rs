//! Loop/one-shot and tempo columns of the `analysis` table (T3.4,
//! ADR-021): `kind`, `kind_conf`, `bpm`, `bpm_conf`, `bpm_source`.
//!
//! [`upsert_kind_tempo`] writes only these five columns, so other analysis
//! results (key, pitch, category; later tasks) are left untouched; a new row
//! has them NULL. A one-shot (or a loop without tempo) stores `bpm` NULL,
//! clearing any earlier value. User overrides live in `overrides` and are
//! never written here.

use rusqlite::{params, Connection, OptionalExtension};

use super::Store;
use crate::classify::{Kind, KindResult, KindSource};
use crate::tempo::{Tempo, TempoSource};
use crate::Result;

/// The kind and tempo of one file as stored.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KindTempo {
    /// Loop or one-shot; `None` when there was no evidence (silent audio,
    /// no name hint: [`KindSource::Fallback`]).
    pub kind: Option<Kind>,
    /// Confidence of `kind` (`0..=1`).
    pub kind_conf: Option<f32>,
    /// Tempo (loops only).
    pub bpm: Option<f32>,
    /// Confidence of `bpm` (`0..=1`).
    pub bpm_conf: Option<f32>,
    /// Where `bpm` came from.
    pub bpm_source: Option<TempoSource>,
}

impl KindTempo {
    /// Row values for a classification and (loops only) tempo. A tempo is
    /// dropped unless `kind` is a loop, so one-shots never store a BPM.
    pub fn new(kind: &KindResult, tempo: Option<&Tempo>) -> Self {
        let known = kind.source != KindSource::Fallback;
        let tempo = tempo.filter(|t| known && kind.kind == Kind::Loop && t.bpm > 0.0);
        Self {
            kind: known.then_some(kind.kind),
            kind_conf: known.then_some(kind.confidence),
            bpm: tempo.map(|t| t.bpm),
            bpm_conf: tempo.map(|t| t.confidence),
            bpm_source: tempo.map(|t| t.source),
        }
    }
}

fn kind_str(k: Kind) -> &'static str {
    match k {
        Kind::Loop => "loop",
        Kind::OneShot => "one_shot",
    }
}

/// A confidence the table's CHECK accepts (finite, clamped to `0..=1`).
fn conf(v: Option<f32>) -> Option<f64> {
    v.filter(|c| c.is_finite())
        .map(|c| f64::from(c.clamp(0.0, 1.0)))
}

/// Inserts the five kind/tempo columns of `file_id`, or updates only those
/// on an existing row. A non-finite or non-positive BPM is stored as NULL
/// (with its confidence and source).
///
/// # Errors
/// [`crate::Error::Sqlite`], e.g. a foreign-key violation when `file_id`
/// has no `files` row.
pub fn upsert_kind_tempo(conn: &Connection, file_id: i64, v: &KindTempo) -> Result<()> {
    let bpm = v.bpm.filter(|b| b.is_finite() && *b > 0.0).map(f64::from);
    let (bpm_conf, bpm_source) = match bpm {
        Some(_) => (conf(v.bpm_conf), v.bpm_source.map(TempoSource::as_str)),
        None => (None, None),
    };
    conn.prepare_cached(
        "INSERT INTO analysis (file_id, kind, kind_conf, bpm, bpm_conf, bpm_source) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
         ON CONFLICT (file_id) DO UPDATE SET \
           kind = excluded.kind, kind_conf = excluded.kind_conf, bpm = excluded.bpm, \
           bpm_conf = excluded.bpm_conf, bpm_source = excluded.bpm_source",
    )?
    .execute(params![
        file_id,
        v.kind.map(kind_str),
        v.kind.and(conf(v.kind_conf)),
        bpm,
        bpm_conf,
        bpm_source,
    ])?;
    Ok(())
}

/// The kind/tempo columns of `file_id`, if it has an `analysis` row.
///
/// # Errors
/// [`crate::Error::Sqlite`]. (`kind` and `bpm_source` are CHECKed by the
/// schema, so other values cannot occur; they would read as `None`.)
pub fn get_kind_tempo(conn: &Connection, file_id: i64) -> Result<Option<KindTempo>> {
    type Raw = (
        Option<String>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<String>,
    );
    let raw: Option<Raw> = conn
        .query_row(
            "SELECT kind, kind_conf, bpm, bpm_conf, bpm_source FROM analysis WHERE file_id = ?1",
            [file_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?;
    let Some((kind, kind_conf, bpm, bpm_conf, source)) = raw else {
        return Ok(None);
    };
    let kind = match kind.as_deref() {
        Some("loop") => Some(Kind::Loop),
        Some("one_shot") => Some(Kind::OneShot),
        _ => None,
    };
    let bpm_source = match source.as_deref() {
        Some("detected") => Some(TempoSource::Detected),
        Some("filename") => Some(TempoSource::Filename),
        _ => None,
    };
    let f = |v: Option<f64>| v.map(|x| x as f32);
    Ok(Some(KindTempo {
        kind,
        kind_conf: f(kind_conf),
        bpm: f(bpm),
        bpm_conf: f(bpm_conf),
        bpm_source,
    }))
}

impl Store {
    /// [`upsert_kind_tempo`] on this store's connection.
    ///
    /// # Errors
    /// See [`upsert_kind_tempo`].
    pub fn put_kind_tempo(&self, file_id: i64, v: &KindTempo) -> Result<()> {
        upsert_kind_tempo(self.conn(), file_id, v)
    }

    /// [`get_kind_tempo`] on this store's connection.
    ///
    /// # Errors
    /// See [`get_kind_tempo`].
    pub fn kind_tempo(&self, file_id: i64) -> Result<Option<KindTempo>> {
        get_kind_tempo(self.conn(), file_id)
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

    fn loop_row() -> KindTempo {
        KindTempo {
            kind: Some(Kind::Loop),
            kind_conf: Some(0.9),
            bpm: Some(122.5),
            bpm_conf: Some(0.6),
            bpm_source: Some(TempoSource::Detected),
        }
    }

    #[test]
    fn round_trip_and_one_shot_clears_bpm() {
        let (store, id) = store_with_file();
        assert_eq!(store.kind_tempo(id).unwrap(), None);
        store.put_kind_tempo(id, &loop_row()).unwrap();
        assert_eq!(store.kind_tempo(id).unwrap(), Some(loop_row()));
        let one_shot = KindTempo {
            kind: Some(Kind::OneShot),
            kind_conf: Some(0.97),
            bpm: None,
            bpm_conf: None,
            bpm_source: None,
        };
        store.put_kind_tempo(id, &one_shot).unwrap();
        assert_eq!(store.kind_tempo(id).unwrap(), Some(one_shot));
    }

    #[test]
    fn other_analysis_columns_are_untouched() {
        let (store, id) = store_with_file();
        store
            .conn()
            .execute(
                "INSERT INTO analysis (file_id, key, key_conf, key_source, category) \
                 VALUES (?1, 'Am', 0.8, 'detected', 'drums')",
                [id],
            )
            .unwrap();
        store.put_kind_tempo(id, &loop_row()).unwrap();
        let (key, cat): (String, String) = store
            .conn()
            .query_row(
                "SELECT key, category FROM analysis WHERE file_id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((key.as_str(), cat.as_str()), ("Am", "drums"));
    }

    #[test]
    fn out_of_range_values_are_clamped_or_nulled() {
        let (store, id) = store_with_file();
        let bad = KindTempo {
            kind_conf: Some(1.5),
            bpm: Some(f32::NAN),
            ..loop_row()
        };
        store.put_kind_tempo(id, &bad).unwrap();
        let got = store.kind_tempo(id).unwrap().unwrap();
        assert_eq!(got.kind_conf, Some(1.0));
        assert_eq!((got.bpm, got.bpm_conf, got.bpm_source), (None, None, None));
        assert!(store.put_kind_tempo(999, &loop_row()).is_err());
    }
}
