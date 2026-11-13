//! User data, part 3 (T5.5b, ADR-031): overrides of BPM, key, category and
//! kind. Effective value = override ?? detected ?? filename prior; the text
//! index reads the `overrides` rows (`index/source.rs`), analysis never
//! writes them.
//!
//! Values are validated and normalized here so that what is stored is
//! exactly what the index parses:
//! - `bpm`: a plain decimal number (`128`, `87.5`; no unit, no sign, no
//!   exponent) in [`MIN_BPM`]`..=`[`MAX_BPM`], rounded to 0.01, stored REAL;
//! - `key`: anything [`parse_key`] reads (`Am`, `a minor`, `8A`, `F#`),
//!   stored in the canonical `Key` spelling analysis uses;
//! - `category`: a taxonomy id or a spelling [`Category::parse`] knows
//!   (`hi-hat`), stored as the id;
//! - `kind`: `loop` or `one_shot` (`one-shot`, `oneshot`, `one shot` accepted).
//!
//! Clearing an override deletes the row, so the detected value shows again.

use rusqlite::params;
use rusqlite::types::Value;

use super::user_data::require_files;
use super::Store;
use crate::classify::Category;
use crate::search::keys::parse_key;
use crate::{Error, Result};

/// Smallest BPM an override may have.
pub const MIN_BPM: f64 = 20.0;
/// Largest BPM an override may have.
pub const MAX_BPM: f64 = 400.0;

/// An overridable field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverrideField {
    /// Tempo (plain number).
    Bpm,
    /// Musical key.
    Key,
    /// Category id of the taxonomy.
    Category,
    /// `loop` or `one_shot`.
    Kind,
}

impl OverrideField {
    /// Every field.
    pub const ALL: [Self; 4] = [Self::Bpm, Self::Key, Self::Category, Self::Kind];

    /// Column / `overrides.field` name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bpm => "bpm",
            Self::Key => "key",
            Self::Category => "category",
            Self::Kind => "kind",
        }
    }

    /// Parses a field name (case-insensitive).
    ///
    /// # Errors
    /// [`Error::InvalidInput`] naming the accepted fields.
    pub fn parse(s: &str) -> Result<Self> {
        let s = s.trim().to_ascii_lowercase();
        Self::ALL
            .into_iter()
            .find(|f| f.as_str() == s)
            .ok_or_else(|| Error::InvalidInput {
                reason: format!("unknown override field '{s}' (use bpm, key, category or kind)"),
            })
    }

    /// Validates and normalizes `raw` for this field (see the module docs).
    ///
    /// # Errors
    /// [`Error::InvalidInput`] saying what is accepted.
    pub fn normalize(self, raw: &str) -> Result<Value> {
        let raw = raw.trim();
        let bad = |what: &str| Error::InvalidInput {
            reason: format!("'{raw}' is not {what}"),
        };
        match self {
            Self::Bpm => {
                let plain = !raw.is_empty()
                    && raw.chars().all(|c| c.is_ascii_digit() || c == '.')
                    && raw.chars().filter(|&c| c == '.').count() <= 1
                    && raw.chars().any(|c| c.is_ascii_digit());
                let bpm = raw.parse::<f64>().ok().filter(|_| plain);
                match bpm {
                    Some(b) if (MIN_BPM..=MAX_BPM).contains(&b) => {
                        Ok(Value::Real((b * 100.0).round() / 100.0))
                    }
                    Some(_) => Err(bad(&format!(
                        "a BPM between {MIN_BPM} and {MAX_BPM} (a plain number)"
                    ))),
                    None => Err(bad("a plain number (e.g. 128 or 87.5, no unit)")),
                }
            }
            Self::Key => parse_key(raw)
                .map(|k| Value::Text(k.to_string()))
                .ok_or_else(|| bad("a key (e.g. Am, F#, Bb minor, 8A)")),
            Self::Category => Category::parse(raw)
                .map(|c| Value::Text(c.as_str().to_owned()))
                .ok_or_else(|| bad("a category (see docs/categories.md)")),
            Self::Kind => match raw.to_ascii_lowercase().replace(['-', ' '], "_").as_str() {
                "loop" => Ok(Value::Text("loop".into())),
                "one_shot" | "oneshot" => Ok(Value::Text("one_shot".into())),
                _ => Err(bad("a kind (loop or one_shot)")),
            },
        }
    }
}

impl Store {
    /// Sets the override `field` of every file in `ids` to the normalized
    /// `raw` value (all-or-nothing; the value is validated first). Returns
    /// the number of files whose override changed.
    ///
    /// # Errors
    /// [`Error::InvalidInput`], [`Error::FileNotFound`], [`Error::Sqlite`].
    pub fn set_override(&mut self, ids: &[i64], field: OverrideField, raw: &str) -> Result<usize> {
        let value = field.normalize(raw)?;
        let tx = self.conn_mut().transaction()?;
        require_files(&tx, ids)?;
        let mut changed = 0;
        {
            // `IS NOT` so re-setting the same value does not count.
            let mut st = tx.prepare_cached(
                "INSERT INTO overrides (file_id, field, value) VALUES (?1, ?2, ?3) \
                 ON CONFLICT (file_id, field) DO UPDATE SET value = excluded.value \
                 WHERE overrides.value IS NOT excluded.value",
            )?;
            for &id in ids {
                changed += st.execute(params![id, field.as_str(), value])?;
            }
        }
        tx.commit()?;
        Ok(changed)
    }

    /// Removes the override `field` of every file in `ids` (the detected
    /// value applies again). Returns the number of overrides removed.
    ///
    /// # Errors
    /// [`Error::FileNotFound`], [`Error::Sqlite`].
    pub fn clear_override(&mut self, ids: &[i64], field: OverrideField) -> Result<usize> {
        let tx = self.conn_mut().transaction()?;
        require_files(&tx, ids)?;
        let mut changed = 0;
        {
            let mut st =
                tx.prepare_cached("DELETE FROM overrides WHERE file_id = ?1 AND field = ?2")?;
            for &id in ids {
                changed += st.execute(params![id, field.as_str()])?;
            }
        }
        tx.commit()?;
        Ok(changed)
    }

    /// The overrides of one file as `(field, value)`, by field name. A
    /// cleared-to-nothing row (`NULL`, written by nothing in this build)
    /// shows as an empty value.
    ///
    /// # Errors
    /// [`Error::Sqlite`].
    pub fn overrides_of(&self, file_id: i64) -> Result<Vec<(String, String)>> {
        let mut st = self.conn().prepare_cached(
            "SELECT field, COALESCE(CAST(value AS TEXT), '') FROM overrides \
             WHERE file_id = ?1 ORDER BY field",
        )?;
        let rows = st
            .query_map([file_id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::user_data::store_with_files;

    fn text(v: Value) -> String {
        match v {
            Value::Text(s) => s,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn bpm_is_a_plain_positive_number() {
        let ok = |s: &str| OverrideField::Bpm.normalize(s).ok();
        assert_eq!(ok("128"), Some(Value::Real(128.0)));
        assert_eq!(ok(" 87.5 "), Some(Value::Real(87.5)));
        assert_eq!(ok("90.456"), Some(Value::Real(90.46)));
        for bad in [
            "", "128 bpm", "bpm", "-120", "+120", "1e2", "NaN", "inf", "0", "1.2.3", ".", "19.9",
            "400.1", "12,5",
        ] {
            let err = OverrideField::Bpm.normalize(bad).unwrap_err();
            assert!(matches!(err, Error::InvalidInput { .. }), "{bad}: {err}");
        }
    }

    #[test]
    fn key_category_kind_are_canonical() {
        let key = |s: &str| OverrideField::Key.normalize(s).map(text);
        assert_eq!(key("a minor").unwrap(), key("Am").unwrap());
        assert_eq!(key("8A").unwrap(), key("am").unwrap());
        assert!(key("H#").is_err() && key("").is_err());
        let cat = |s: &str| OverrideField::Category.normalize(s).map(text);
        assert_eq!(cat("Hi-Hat").unwrap(), "hat");
        assert_eq!(cat("kick").unwrap(), "kick");
        assert!(cat("guitar").is_err());
        let kind = |s: &str| OverrideField::Kind.normalize(s).map(text);
        assert_eq!(kind("One-Shot").unwrap(), "one_shot");
        assert_eq!(kind("loop").unwrap(), "loop");
        assert!(kind("sample").is_err());
        assert!(OverrideField::parse("Tempo").is_err());
        assert_eq!(OverrideField::parse(" BPM ").unwrap(), OverrideField::Bpm);
    }

    #[test]
    fn set_replaces_counts_changes_and_clear_reverts() {
        let mut s = store_with_files();
        assert_eq!(
            s.set_override(&[1, 2], OverrideField::Bpm, "128").unwrap(),
            2
        );
        assert_eq!(
            s.set_override(&[1, 2], OverrideField::Bpm, "128").unwrap(),
            0
        );
        assert_eq!(s.set_override(&[1], OverrideField::Bpm, "100").unwrap(), 1);
        s.set_override(&[1], OverrideField::Kind, "loop").unwrap();
        let shown = s.overrides_of(1).unwrap();
        assert_eq!(shown[0], ("bpm".into(), "100.0".into()));
        assert_eq!(shown[1], ("kind".into(), "loop".into()));
        assert_eq!(s.clear_override(&[1, 2, 3], OverrideField::Bpm).unwrap(), 2);
        assert_eq!(s.overrides_of(1).unwrap().len(), 1);
    }

    #[test]
    fn invalid_value_or_file_changes_nothing() {
        let mut s = store_with_files();
        assert!(s.set_override(&[1], OverrideField::Bpm, "fast").is_err());
        assert!(matches!(
            s.set_override(&[1, 77], OverrideField::Bpm, "120"),
            Err(Error::FileNotFound { id: 77 })
        ));
        assert!(s.overrides_of(1).unwrap().is_empty());
    }
}
