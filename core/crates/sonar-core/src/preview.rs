//! Preview loudness normalization (T6.4, ADR-034): the gain the app applies
//! so that auditioning files of very different levels is comfortable.
//!
//! Target is -16 LUFS integrated (ITU-R BS.1770 measurement stored in
//! `audio_meta`), limited so the sample peak stays at or below -1 dBFS, and
//! clamped to a sane range. The app converts dB to linear and applies it as
//! the player volume; it never touches the file.

/// Integrated loudness the preview aims for (LUFS).
pub const TARGET_LUFS: f64 = -16.0;
/// Highest sample peak the preview may reach after gain (dBFS).
pub const PEAK_CEILING_DB: f64 = -1.0;
/// Peak level aimed for when loudness is unknown (dBFS).
pub const FALLBACK_PEAK_DB: f64 = -6.0;
/// Lowest gain (dB).
pub const MIN_GAIN_DB: f64 = -24.0;
/// Highest gain (dB).
pub const MAX_GAIN_DB: f64 = 12.0;

/// Preview gain in dB for a file with the given integrated loudness and
/// sample peak, or `None` when neither value is usable (play at 0 dB).
///
/// - loudness known: `-16 - loudness`, capped by `-1 - peak` if the peak is
///   known;
/// - loudness missing (too short for BS.1770 gating) but peak known:
///   `-6 - peak`;
/// - result clamped to [-24, +12] dB.
///
/// Non-finite inputs count as missing.
#[must_use]
pub fn preview_gain_db(loudness_lufs: Option<f64>, peak_db: Option<f64>) -> Option<f64> {
    let loudness = loudness_lufs.filter(|v| v.is_finite());
    let peak = peak_db.filter(|v| v.is_finite());
    let gain = match (loudness, peak) {
        (Some(l), Some(p)) => (TARGET_LUFS - l).min(PEAK_CEILING_DB - p),
        (Some(l), None) => TARGET_LUFS - l,
        (None, Some(p)) => FALLBACK_PEAK_DB - p,
        (None, None) => return None,
    };
    Some(gain.clamp(MIN_GAIN_DB, MAX_GAIN_DB))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Option<f64>, b: f64) {
        let a = a.expect("some");
        assert!((a - b).abs() < 1e-9, "{a} != {b}");
    }

    #[test]
    fn loud_loop_is_attenuated() {
        close(preview_gain_db(Some(-8.0), Some(-0.3)), -8.0);
    }

    #[test]
    fn quiet_one_shot_is_ceiling_limited() {
        // wants +10 dB, but the -4 dB peak only allows +3.
        close(preview_gain_db(Some(-26.0), Some(-4.0)), 3.0);
    }

    #[test]
    fn loudness_only_uses_target() {
        close(preview_gain_db(Some(-20.0), None), 4.0);
    }

    #[test]
    fn missing_loudness_falls_back_to_peak() {
        close(preview_gain_db(None, Some(-12.0)), 6.0);
        close(preview_gain_db(Some(f64::NAN), Some(-12.0)), 6.0);
    }

    #[test]
    fn clamps() {
        close(preview_gain_db(Some(10.0), None), MIN_GAIN_DB);
        close(preview_gain_db(Some(-60.0), None), MAX_GAIN_DB);
        close(preview_gain_db(None, Some(-75.0)), MAX_GAIN_DB);
    }

    #[test]
    fn nothing_usable_is_none() {
        assert_eq!(preview_gain_db(None, None), None);
        assert_eq!(
            preview_gain_db(Some(f64::NEG_INFINITY), Some(f64::NAN)),
            None
        );
    }
}
