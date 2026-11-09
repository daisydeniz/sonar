//! Mono downmix: the arithmetic mean of all channels.
//!
//! Equal-weight fold-down: mean (not sum) keeps a stereo file with
//! identical channels at its original level and can never clip. Anti-phase
//! content cancels, as in any passive mono fold-down; analysis features
//! accept that (sample libraries are overwhelmingly mono-compatible).

/// Appends the per-frame mean of `planes[..][..frames]` to `out`.
///
/// Every plane must hold at least `frames` samples; shorter planes are
/// treated as if `frames` were their length (no panic). With no planes,
/// nothing is appended. Allocation-free when `out` has spare capacity.
pub fn append_mean(planes: &[Vec<f32>], frames: usize, out: &mut Vec<f32>) {
    let frames = planes
        .iter()
        .map(Vec::len)
        .min()
        .map_or(0, |shortest| shortest.min(frames));
    let Some((first, rest)) = planes.split_first() else {
        return;
    };
    let start = out.len();
    out.extend_from_slice(&first[..frames]);
    if rest.is_empty() {
        return;
    }
    let acc = &mut out[start..];
    for plane in rest {
        // Zipped slices of equal length: the compiler vectorises this.
        for (a, s) in acc.iter_mut().zip(&plane[..frames]) {
            *a += *s;
        }
    }
    let scale = 1.0 / planes.len() as f32;
    for a in acc.iter_mut() {
        *a *= scale;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_is_copied_verbatim() {
        let mut out = vec![9.0];
        append_mean(&[vec![0.5, -0.25, 1.0]], 3, &mut out);
        assert_eq!(out, [9.0, 0.5, -0.25, 1.0]);
    }

    #[test]
    fn stereo_is_averaged() {
        let mut out = Vec::new();
        append_mean(&[vec![1.0, 0.5, 0.0], vec![1.0, -0.5, 0.5]], 3, &mut out);
        assert_eq!(out, [1.0, 0.0, 0.25]);
    }

    #[test]
    fn identical_channels_keep_level() {
        let tone: Vec<f32> = (0..64).map(|i| (i as f32 * 0.3).sin()).collect();
        let mut out = Vec::new();
        append_mean(&[tone.clone(), tone.clone(), tone.clone()], 64, &mut out);
        for (o, t) in out.iter().zip(&tone) {
            assert!((o - t).abs() < 1e-6);
        }
    }

    #[test]
    fn frames_limits_output_and_short_planes_do_not_panic() {
        let mut out = Vec::new();
        append_mean(&[vec![1.0; 10], vec![0.0; 10]], 4, &mut out);
        assert_eq!(out, [0.5; 4]);
        out.clear();
        append_mean(&[vec![1.0; 10], vec![1.0; 2]], 8, &mut out);
        assert_eq!(out.len(), 2);
        out.clear();
        append_mean(&[], 8, &mut out);
        assert!(out.is_empty());
    }
}
