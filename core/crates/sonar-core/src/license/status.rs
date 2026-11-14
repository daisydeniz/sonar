//! Trial and license status for a data directory, and activation.

use std::path::Path;

use super::state::{self, State};
use super::verify::normalize_key;
use super::{Date, LicenseError, LicenseInfo, LicenseVerifier, SignedKeyVerifier};

/// Length of the free trial.
pub const TRIAL_DAYS: i64 = 14;
const DAY: i64 = 86_400;
/// `last_seen` is rewritten only when it advanced by more than this, so
/// status polling does not hit the disk every call.
const LAST_SEEN_WRITE_STEP: i64 = 60;

/// Where the user stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LicenseStatus {
    /// Trial running.
    Trial {
        /// Whole days left, rounded up (14 on day 0, 1 on the last day).
        days_left: u32,
        /// Unix seconds when the trial ends.
        ends_at: i64,
    },
    /// Trial over, no valid license.
    TrialExpired {
        /// Unix seconds when the trial ended.
        ended_at: i64,
    },
    /// A valid license is activated.
    Licensed {
        /// What the key says.
        info: LicenseInfo,
    },
    /// An activated subscription whose last day has passed.
    LicenseExpired {
        /// What the key says (`info.expires` is the lapsed date).
        info: LicenseInfo,
    },
}

/// Unix seconds now (0 if the system clock is before 1970).
pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .unwrap_or(0)
}

/// [`status_with`] using this build's public key.
///
/// # Errors
/// [`LicenseError::BadSigningKey`] (broken build), [`LicenseError::Io`].
pub fn status(data_dir: &Path, now: i64) -> Result<LicenseStatus, LicenseError> {
    status_with(&SignedKeyVerifier::builtin()?, data_dir, now)
}

/// [`activate_with`] using this build's public key.
///
/// # Errors
/// See [`activate_with`].
pub fn activate(data_dir: &Path, key: &str, now: i64) -> Result<LicenseInfo, LicenseError> {
    activate_with(&SignedKeyVerifier::builtin()?, data_dir, key, now)
}

/// Current status. The first call in a data directory starts the trial.
///
/// Effective time is `max(now, last_seen)`, so setting the clock back never
/// adds trial days. A stored key that no longer verifies (other public key,
/// damaged) is ignored: the status then follows the trial dates. A corrupt
/// state file is never an error (see [`state`]'s `load`).
///
/// # Errors
/// [`LicenseError::Io`] if the state file cannot be written.
pub fn status_with(
    verifier: &dyn LicenseVerifier,
    data_dir: &Path,
    now: i64,
) -> Result<LicenseStatus, LicenseError> {
    let loaded = state::load(data_dir, now);
    let mut st = loaded.state;
    let eff = now.max(st.last_seen);
    if loaded.fresh || eff - st.last_seen > LAST_SEEN_WRITE_STEP {
        st.last_seen = eff;
        state::save(data_dir, &st)?;
    }
    Ok(compute(verifier, &st, eff))
}

fn compute(verifier: &dyn LicenseVerifier, st: &State, eff: i64) -> LicenseStatus {
    if let Some(info) = st.key.as_deref().and_then(|k| verifier.inspect(k).ok()) {
        return if info.is_expired_on(Date::from_unix(eff)) {
            LicenseStatus::LicenseExpired { info }
        } else {
            LicenseStatus::Licensed { info }
        };
    }
    let ends_at = st.trial_started.saturating_add(TRIAL_DAYS * DAY);
    if eff < ends_at {
        let days_left = u32::try_from((ends_at - eff + DAY - 1) / DAY).unwrap_or(0);
        LicenseStatus::Trial { days_left, ends_at }
    } else {
        LicenseStatus::TrialExpired { ended_at: ends_at }
    }
}

/// Verifies `key` and stores it. An invalid or expired key is rejected and
/// whatever was stored stays untouched.
///
/// # Errors
/// The key errors of [`LicenseVerifier::verify`], [`LicenseError::Io`].
pub fn activate_with(
    verifier: &dyn LicenseVerifier,
    data_dir: &Path,
    key: &str,
    now: i64,
) -> Result<LicenseInfo, LicenseError> {
    let loaded = state::load(data_dir, now);
    let mut st = loaded.state;
    let eff = now.max(st.last_seen);
    let info = verifier.verify(key, Date::from_unix(eff))?;
    st.last_seen = eff;
    st.key = Some(normalize_key(key));
    state::save(data_dir, &st)?;
    Ok(info)
}

/// Removes the stored key; the trial clock stays as it is. No state file
/// means nothing to do (and does not start a trial).
///
/// # Errors
/// [`LicenseError::Io`] if the state file cannot be written.
pub fn deactivate(data_dir: &Path) -> Result<(), LicenseError> {
    let loaded = state::load(data_dir, now_unix());
    if loaded.fresh && !state::path(data_dir).exists() {
        return Ok(());
    }
    let mut st = loaded.state;
    st.key = None;
    state::save(data_dir, &st)
}
