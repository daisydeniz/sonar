//! Display rows (T5.6): what a result list shows for a file, with effective
//! values (ADR-018: user override, else analysis, else filename prior) read
//! from the store with the same query the text index uses, so a row never
//! disagrees with what search filtered on.

use std::path::{Path, PathBuf};

use super::{lock, Engine};
use crate::index::file_doc;
use crate::key::{fuse, KeySource};
use crate::text::{filename_prior, Key};
use crate::{Result, Store};

/// Everything a result row shows for one file.
#[derive(Debug, Clone, PartialEq)]
pub struct FileRow {
    /// `files.id`.
    pub file_id: i64,
    /// Library the file belongs to.
    pub library_id: i64,
    /// Absolute path.
    pub path: PathBuf,
    /// File name without its media extension.
    pub name: String,
    /// Library folder name and the folders below it, `/`-separated.
    pub folders: String,
    /// Effective category (`kick`, ...), if known.
    pub category: Option<String>,
    /// Effective kind (`loop` / `one_shot`), if known.
    pub kind: Option<String>,
    /// Effective tempo, if known.
    pub bpm: Option<f64>,
    /// Effective key as text (`C#m`), if known.
    pub key: Option<String>,
    /// Confidence of `key` (`0..=1`; a user override is 1, a name-only key
    /// of an unanalysed file the name's own strength); `None` without key.
    pub key_conf: Option<f32>,
    /// What decided `key`: `detected` (audio), `filename`, `both`, or
    /// `user` (override); `None` without key.
    pub key_source: Option<String>,
    /// The filename's key when it disagrees with `key` (`key_source`
    /// `detected`: the audio outweighed the name); `None` otherwise.
    pub key_name: Option<String>,
    /// The audio's key when the filename outweighed a disagreeing audio
    /// candidate (`key_source` `filename`); `None` otherwise.
    pub key_audio: Option<String>,
    /// Note name of a pitched one-shot (`E2`, middle C = `C4`).
    pub note_name: Option<String>,
    /// Cents of the measured pitch from `note_name` (`-50..=50`), if stored.
    pub note_cents: Option<f32>,
    /// Playable length (ms), once the meta stage ran.
    pub duration_ms: Option<u64>,
    /// Sample rate of the stored format (Hz), once the meta stage ran.
    pub sample_rate: Option<u32>,
    /// Channel count of the stored format, once the meta stage ran.
    pub channels: Option<u16>,
    /// Preview gain (dB, ADR-034); `None` until the meta stage ran or when
    /// no level is usable (play at 0 dB).
    pub preview_gain_db: Option<f64>,
    /// In the user's favourites.
    pub favorite: bool,
    /// Hidden by the user.
    pub hidden: bool,
    /// User tags.
    pub tags: Vec<String>,
}

fn load(store: &Store, id: i64) -> Result<Option<FileRow>> {
    let Some(doc) = file_doc(store, id)? else {
        return Ok(None);
    };
    let Some(path) = store.file_path(id)? else {
        return Ok(None);
    };
    let meta = store.audio_meta(id)?;
    let info = KeyInfo::load(store, id, &path, doc.key.is_some())?;
    Ok(Some(FileRow {
        file_id: doc.file_id,
        library_id: doc.library_id,
        path,
        name: doc.name,
        folders: doc.folders,
        category: doc.category,
        kind: doc.kind,
        bpm: doc.bpm,
        key: doc.key.map(|k| k.to_string()),
        key_conf: info.conf,
        key_source: info.source,
        key_name: info.name,
        key_audio: info.audio,
        note_name: info.note_name,
        note_cents: info.note_cents,
        duration_ms: doc.duration_ms,
        sample_rate: meta.as_ref().map(|m| m.sample_rate),
        channels: meta.as_ref().map(|m| m.channels),
        preview_gain_db: meta
            .as_ref()
            .and_then(|m| crate::preview::preview_gain_db(m.loudness_lufs, m.peak_dbfs)),
        favorite: doc.favorite,
        hidden: doc.hidden,
        tags: doc.tags,
    }))
}

/// Key provenance of a row (T10.6, ADR-041), read from the stored analysis.
#[derive(Default)]
struct KeyInfo {
    conf: Option<f32>,
    source: Option<String>,
    name: Option<String>,
    audio: Option<String>,
    note_name: Option<String>,
    note_cents: Option<f32>,
}

impl KeyInfo {
    fn load(store: &Store, id: i64, path: &Path, has_key: bool) -> Result<Self> {
        let mut info = Self::default();
        let kp = store.key_pitch(id)?.unwrap_or_default();
        if let Some(midi) = kp.pitch_midi {
            info.note_name = Some(note_name(midi));
            info.note_cents = kp.pitch_cents;
        }
        if !has_key {
            return Ok(info);
        }
        let overridden = store
            .overrides_of(id)?
            .iter()
            .any(|(field, _)| field == "key");
        let prior = path
            .file_name()
            .and_then(|n| filename_prior(Path::new(n)).key);
        if overridden {
            (info.conf, info.source) = (Some(1.0), Some("user".to_owned()));
        } else if kp.key.is_some() {
            info.conf = Some(kp.key_conf.unwrap_or(0.0));
            info.source = Some(
                kp.key_source
                    .map_or("detected", KeySource::as_str)
                    .to_owned(),
            );
            if kp.key_source == Some(KeySource::Filename) {
                info.audio = kp.key_audio.map(|k| k.to_string());
            }
            // Audio outweighed a name that says something else: show both.
            if kp.key_source == Some(KeySource::Detected) {
                info.name = prior
                    .filter(|p| kp.key.is_some_and(|k| !fuse::matches_prior(k, p)))
                    .map(|p| p.key.to_string());
            }
        } else if let Some(est) = prior.and_then(|p| fuse::fuse(None, Some(p))) {
            (info.conf, info.source) = (Some(est.confidence), Some("filename".to_owned()));
        }
        Ok(info)
    }
}

/// Scientific pitch name of a MIDI note (`40` → `E2`, `60` → `C4`).
fn note_name(midi: u8) -> String {
    let pc = Key {
        root: midi % 12,
        mode: None,
    };
    format!("{pc}{}", i32::from(midi / 12) - 1)
}

impl Engine {
    /// The display row of file `id`; `None` if the id is unknown or the
    /// file is marked missing.
    ///
    /// # Errors
    /// [`crate::Error::Sqlite`].
    pub fn row(&self, id: i64) -> Result<Option<FileRow>> {
        load(&lock(&self.inner.store), id)
    }

    /// Rows for `ids` in the order given (one store lock for the whole
    /// page); unknown or missing ids are skipped.
    ///
    /// # Errors
    /// [`crate::Error::Sqlite`].
    pub fn rows(&self, ids: &[i64]) -> Result<Vec<FileRow>> {
        let store = lock(&self.inner.store);
        let mut out = Vec::with_capacity(ids.len());
        for &id in ids {
            out.extend(load(&store, id)?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::KeySource;
    use crate::store::key_pitch::KeyPitch;

    #[test]
    fn note_names() {
        assert_eq!(note_name(40), "E2");
        assert_eq!(note_name(60), "C4");
        assert_eq!(note_name(69), "A4");
        assert_eq!(note_name(0), "C-1");
        assert_eq!(note_name(49), "C#3");
    }

    fn info(name: &str, kp: &KeyPitch, overridden: bool) -> KeyInfo {
        let mut store = Store::open_in_memory().unwrap();
        store
            .conn()
            .execute_batch(
                "INSERT INTO libraries (id, root_path, added_at) VALUES (1, '/lib', 0);
                 INSERT INTO files (id, library_id, rel_path, size, mtime)
                   VALUES (7, 1, 'a.wav', 10, 0);",
            )
            .unwrap();
        store.put_key_pitch(7, kp).unwrap();
        if overridden {
            store
                .set_override(&[7], crate::store::OverrideField::Key, "Dm")
                .unwrap();
        }
        let path = PathBuf::from("/lib").join(name);
        KeyInfo::load(&store, 7, &path, true).unwrap()
    }

    fn detected(key: &str, source: KeySource) -> KeyPitch {
        KeyPitch {
            key: crate::search::keys::parse_key(key),
            key_conf: Some(0.8),
            key_source: Some(source),
            pitch_midi: Some(40),
            pitch_cents: Some(32.0),
            ..KeyPitch::default()
        }
    }

    #[test]
    fn disagreement_only_when_audio_won_over_a_different_name() {
        let d = info("Pad Am.wav", &detected("Cm", KeySource::Detected), false);
        assert_eq!(d.name.as_deref(), Some("Am"));
        assert_eq!(d.source.as_deref(), Some("detected"));
        assert_eq!(d.conf, Some(0.8));
        assert_eq!(
            (d.note_name.as_deref(), d.note_cents),
            (Some("E2"), Some(32.0))
        );
        let agree = info("Pad Cm.wav", &detected("Cm", KeySource::Both), false);
        assert_eq!(agree.name, None);
        // A mode-less name of the same root is no disagreement.
        let same_root = info("Pad C.wav", &detected("Cm", KeySource::Detected), false);
        assert_eq!(same_root.name, None);
    }

    #[test]
    fn name_that_won_keeps_the_audio_key() {
        let kp = KeyPitch {
            key_audio: crate::search::keys::parse_key("Cm"),
            ..detected("Am", KeySource::Filename)
        };
        let n = info("Pad Am.wav", &kp, false);
        assert_eq!(n.audio.as_deref(), Some("Cm"));
        assert_eq!((n.source.as_deref(), n.name), (Some("filename"), None));
        let d = info("Pad Am.wav", &detected("Cm", KeySource::Detected), false);
        assert_eq!(d.audio, None);
    }

    #[test]
    fn override_is_certain_user_sourced() {
        let o = info("Pad Am.wav", &detected("Cm", KeySource::Detected), true);
        assert_eq!(
            (o.conf, o.source.as_deref(), o.name),
            (Some(1.0), Some("user"), None)
        );
    }
}
