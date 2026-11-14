//! Licensing exports (T7.5, ADR-038). Free functions, not methods of
//! `SonarEngine`: the license gate must work even if the engine cannot open
//! (e.g. a damaged database).
//!
//! `home` is the data directory, as for `SonarEngine.open(home:)`; `nil`
//! means the default (`SONAR_HOME` or Application Support). An invalid or
//! expired key is `FfiError.License` with a user-readable message; disk
//! problems are `FfiError.Io`.

use std::path::PathBuf;

use sonar_core::license::{self, LicenseInfo, LicenseStatus};
use sonar_core::{paths, Error};

use crate::error::guard;
use crate::FfiError;

/// Which screen the app should be on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum LicenseStateKind {
    /// Trial running (`days_left`, `ends_at_unix`).
    Trial,
    /// Trial over, no valid license: show the gate.
    TrialExpired,
    /// A valid license is activated.
    Licensed,
    /// An activated subscription has lapsed (`expires`): show the gate.
    LicenseExpired,
}

/// Licensing state for the UI.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct LicenseState {
    /// Which state this is.
    pub kind: LicenseStateKind,
    /// Whole days left in the trial (rounded up); 0 outside `Trial`.
    pub days_left: u32,
    /// Unix seconds the trial ends (`Trial`) or ended (`TrialExpired`).
    pub ends_at_unix: Option<i64>,
    /// Licensee name/e-mail (`Licensed`, `LicenseExpired`).
    pub licensee: Option<String>,
    /// Store/provider license id (`Licensed`, `LicenseExpired`).
    pub license_id: Option<String>,
    /// `"perpetual"` or `"subscription"` (`Licensed`, `LicenseExpired`).
    pub license_kind: Option<String>,
    /// Last valid day `YYYY-MM-DD` of a subscription.
    pub expires: Option<String>,
    /// True when this build trusts the dev key (not a release build).
    pub dev_key: bool,
}

impl LicenseState {
    fn blank(kind: LicenseStateKind) -> Self {
        Self {
            kind,
            days_left: 0,
            ends_at_unix: None,
            licensee: None,
            license_id: None,
            license_kind: None,
            expires: None,
            dev_key: license::uses_dev_key(),
        }
    }

    fn licensed(kind: LicenseStateKind, info: &LicenseInfo) -> Self {
        Self {
            licensee: Some(info.licensee.clone()),
            license_id: Some(info.license_id.clone()),
            license_kind: Some(info.kind.as_str().to_owned()),
            expires: info.expires.map(|d| d.to_string()),
            ..Self::blank(kind)
        }
    }
}

impl From<&LicenseStatus> for LicenseState {
    fn from(s: &LicenseStatus) -> Self {
        match s {
            LicenseStatus::Trial { days_left, ends_at } => Self {
                days_left: *days_left,
                ends_at_unix: Some(*ends_at),
                ..Self::blank(LicenseStateKind::Trial)
            },
            LicenseStatus::TrialExpired { ended_at } => Self {
                ends_at_unix: Some(*ended_at),
                ..Self::blank(LicenseStateKind::TrialExpired)
            },
            LicenseStatus::Licensed { info } => Self::licensed(LicenseStateKind::Licensed, info),
            LicenseStatus::LicenseExpired { info } => {
                Self::licensed(LicenseStateKind::LicenseExpired, info)
            }
        }
    }
}

fn dir(home: Option<String>) -> sonar_core::Result<PathBuf> {
    match home.filter(|h| !h.is_empty()) {
        Some(h) => Ok(PathBuf::from(h)),
        None => paths::data_dir(),
    }
}

/// Current trial/license state; the first call starts the trial.
#[uniffi::export]
pub fn license_status(home: Option<String>) -> Result<LicenseState, FfiError> {
    guard(|| {
        let s = license::status(&dir(home)?, license::now_unix())?;
        Ok(LicenseState::from(&s))
    })
}

/// Verifies and stores a license key (whitespace in a pasted key is fine);
/// returns the new state. An invalid key is `FfiError.License` and changes
/// nothing.
#[uniffi::export]
pub fn activate_license(home: Option<String>, key: String) -> Result<LicenseState, FfiError> {
    guard(|| {
        let d = dir(home)?;
        let now = license::now_unix();
        license::activate(&d, &key, now)?;
        Ok(LicenseState::from(&license::status(&d, now)?))
    })
}

/// Removes the stored key (the trial clock is not reset); returns the new
/// state.
#[uniffi::export]
pub fn deactivate_license(home: Option<String>) -> Result<LicenseState, FfiError> {
    guard(|| {
        let d = dir(home)?;
        license::deactivate(&d).map_err(Error::from)?;
        Ok(LicenseState::from(&license::status(
            &d,
            license::now_unix(),
        )?))
    })
}

#[cfg(test)]
mod tests {
    use sonar_core::license::dev;
    use sonar_core::license::{issue, Date, LicenseInfo, LicenseKind};

    use super::*;

    fn key(expires: Option<&str>) -> String {
        let info = LicenseInfo {
            licensee: "Ada".into(),
            license_id: "ord_1".into(),
            issued: Date::parse("2019-01-01").expect("date"),
            kind: if expires.is_some() {
                LicenseKind::Subscription
            } else {
                LicenseKind::Perpetual
            },
            expires: expires.map(|e| Date::parse(e).expect("date")),
        };
        issue(&dev::signing_key(), &info).expect("issue")
    }

    fn home(dir: &tempfile::TempDir) -> Option<String> {
        Some(dir.path().to_str().expect("utf-8").to_owned())
    }

    #[test]
    fn fresh_dir_starts_a_trial() {
        let t = tempfile::tempdir().expect("tmp");
        let s = license_status(home(&t)).expect("status");
        assert_eq!(s.kind, LicenseStateKind::Trial);
        assert_eq!(s.days_left, 14);
        assert!(s.ends_at_unix.is_some() && s.licensee.is_none());
    }

    #[test]
    fn activate_then_deactivate() {
        let t = tempfile::tempdir().expect("tmp");
        let s = activate_license(home(&t), format!(" {}\n", key(None))).expect("activate");
        assert_eq!(s.kind, LicenseStateKind::Licensed);
        assert_eq!(s.licensee.as_deref(), Some("Ada"));
        assert_eq!(s.license_kind.as_deref(), Some("perpetual"));
        let s = deactivate_license(home(&t)).expect("deactivate");
        assert_eq!(s.kind, LicenseStateKind::Trial);
    }

    #[test]
    fn invalid_key_is_a_license_error_and_keeps_the_old_key() {
        let t = tempfile::tempdir().expect("tmp");
        activate_license(home(&t), key(None)).expect("activate");
        for bad in ["nonsense".to_owned(), key(Some("2020-01-01"))] {
            match activate_license(home(&t), bad) {
                Err(FfiError::License { message }) => assert!(!message.is_empty()),
                other => panic!("{other:?}"),
            }
        }
        assert_eq!(
            license_status(home(&t)).expect("status").kind,
            LicenseStateKind::Licensed
        );
    }

    #[test]
    fn unwritable_data_dir_is_an_io_error_not_a_panic() {
        let t = tempfile::tempdir().expect("tmp");
        let file = t.path().join("a-file");
        std::fs::write(&file, b"x").expect("write");
        let under_file = file.join("sub");
        let r = license_status(Some(under_file.to_str().expect("utf-8").to_owned()));
        assert!(matches!(r, Err(FfiError::Io { .. })), "{r:?}");
    }
}
