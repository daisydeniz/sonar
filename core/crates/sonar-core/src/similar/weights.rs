//! Per-aspect weights of a similarity query (ADR-024).

use std::fmt;
use std::str::FromStr;

use serde::Serialize;

use crate::dsp::aspects::Aspect;
use crate::index::vectors::RAW_ASPECTS;
use crate::{Error, Result};

/// Weight of each raw aspect in the combined distance (see
/// [`super`] for the formula). Weights are relative: only their ratios
/// matter. The default ("Overall") weighs all four equally, like
/// `OVERALL_WEIGHTS`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct AspectWeights {
    /// Spectrum weight.
    pub spectrum: f32,
    /// Timbre weight.
    pub timbre: f32,
    /// Pitch weight.
    pub pitch: f32,
    /// Amplitude weight.
    pub amplitude: f32,
}

impl Default for AspectWeights {
    /// Overall: every aspect weight 1.
    fn default() -> Self {
        Self::overall(1.0)
    }
}

impl AspectWeights {
    /// Every aspect weighted `w` (= the Overall aspect).
    pub fn overall(w: f32) -> Self {
        Self {
            spectrum: w,
            timbre: w,
            pitch: w,
            amplitude: w,
        }
    }

    /// Only `aspect` (Overall = all four equally).
    pub fn only(aspect: Aspect) -> Self {
        let mut w = Self::overall(0.0);
        w.add(aspect, 1.0);
        w
    }

    /// The weights in [`RAW_ASPECTS`] order.
    pub fn as_array(&self) -> [f32; 4] {
        [self.spectrum, self.timbre, self.pitch, self.amplitude]
    }

    /// Adds `w` to `aspect` (Overall: to each of the four).
    pub fn add(&mut self, aspect: Aspect, w: f32) {
        match aspect {
            Aspect::Spectrum => self.spectrum += w,
            Aspect::Timbre => self.timbre += w,
            Aspect::Pitch => self.pitch += w,
            Aspect::Amplitude => self.amplitude += w,
            Aspect::Overall => {
                for a in RAW_ASPECTS {
                    self.add(a, w);
                }
            }
        }
    }

    /// Weights of the UI's five sliders (T6.6, ADR-035): aspect `a` weighs
    /// `overall + sliders[a]`, i.e. the Overall slider is a flat floor under
    /// all four aspects and each aspect slider adds emphasis on top (the
    /// same additive rule as `overall=1,pitch=1` in [`FromStr`]).
    /// `(1, [0;4])` is the default Overall; `(0, pitch=1)` is pitch only.
    /// `sliders` are in [`RAW_ASPECTS`] order (spectrum, timbre, pitch,
    /// amplitude). The result is [validated](AspectWeights::validate).
    ///
    /// # Errors
    /// [`Error::InvalidQuery`] for a negative or non-finite slider or all
    /// weights zero.
    pub fn from_sliders(overall: f32, sliders: [f32; 4]) -> Result<Self> {
        let [spectrum, timbre, pitch, amplitude] = sliders;
        let w = Self {
            spectrum: overall + spectrum,
            timbre: overall + timbre,
            pitch: overall + pitch,
            amplitude: overall + amplitude,
        };
        let raw = [overall, spectrum, timbre, pitch, amplitude];
        if raw.iter().any(|x| !x.is_finite() || *x < 0.0) {
            return Err(invalid(format!(
                "sliders must be finite and ≥ 0: overall={overall}, {sliders:?}"
            )));
        }
        w.validate()?;
        Ok(w)
    }

    /// Checks that every weight is finite and ≥ 0 and at least one is > 0.
    ///
    /// # Errors
    /// [`Error::InvalidQuery`].
    pub fn validate(&self) -> Result<()> {
        let w = self.as_array();
        if w.iter().any(|x| !x.is_finite() || *x < 0.0) {
            return Err(invalid(format!("weights must be finite and ≥ 0: {self}")));
        }
        if w.iter().sum::<f32>() <= 0.0 {
            return Err(invalid("at least one aspect weight must be > 0".into()));
        }
        Ok(())
    }
}

impl fmt::Display for AspectWeights {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<String> = RAW_ASPECTS
            .iter()
            .zip(self.as_array())
            .map(|(a, w)| format!("{}={w}", a.name()))
            .collect();
        f.write_str(&parts.join(","))
    }
}

impl FromStr for AspectWeights {
    type Err = Error;

    /// `name=weight` pairs separated by commas, names `spectrum`, `timbre`,
    /// `pitch`, `amplitude`, `overall` (adds to all four). Aspects not
    /// listed weigh 0: `pitch=1` is pitch only, `overall=1,pitch=1` doubles
    /// pitch. The result is [validated](AspectWeights::validate).
    fn from_str(s: &str) -> Result<Self> {
        let mut w = Self::overall(0.0);
        for part in s.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            let (name, value) = part
                .split_once('=')
                .ok_or_else(|| invalid(format!("expected name=weight, got {part:?}")))?;
            let aspect = Aspect::ALL
                .into_iter()
                .find(|a| a.name().eq_ignore_ascii_case(name.trim()))
                .ok_or_else(|| {
                    invalid(format!(
                        "unknown aspect {name:?} (spectrum, timbre, pitch, amplitude, overall)"
                    ))
                })?;
            let value: f32 = value
                .trim()
                .parse()
                .map_err(|_| invalid(format!("not a number: {value:?}")))?;
            if !value.is_finite() || value < 0.0 {
                return Err(invalid(format!("weight must be finite and ≥ 0: {part:?}")));
            }
            w.add(aspect, value);
        }
        w.validate()?;
        Ok(w)
    }
}

fn invalid(reason: String) -> Error {
    Error::InvalidQuery { reason }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_named_weights_and_overall() {
        let w: AspectWeights = "pitch=1".parse().unwrap();
        assert_eq!(w.as_array(), [0.0, 0.0, 1.0, 0.0]);
        let w: AspectWeights = " Overall = 1 , pitch=2,".parse().unwrap();
        assert_eq!(w.as_array(), [1.0, 1.0, 3.0, 1.0]);
        assert_eq!(
            AspectWeights::default(),
            AspectWeights::only(Aspect::Overall)
        );
        assert_eq!(AspectWeights::only(Aspect::Timbre).timbre, 1.0);
        assert_eq!(w.to_string(), "spectrum=1,timbre=1,pitch=3,amplitude=1");
    }

    #[test]
    fn sliders_add_to_overall() {
        let d = AspectWeights::from_sliders(1.0, [0.0; 4]).unwrap();
        assert_eq!(d, AspectWeights::default());
        let p = AspectWeights::from_sliders(0.0, [0.0, 0.0, 1.0, 0.0]).unwrap();
        assert_eq!(p, AspectWeights::only(Aspect::Pitch));
        let m = AspectWeights::from_sliders(1.0, [0.0, 0.0, 1.0, 0.0]).unwrap();
        assert_eq!(m.as_array(), [1.0, 1.0, 2.0, 1.0]);
        assert!(AspectWeights::from_sliders(0.0, [0.0; 4]).is_err());
        assert!(AspectWeights::from_sliders(1.0, [-1.0, 0.0, 0.0, 0.0]).is_err());
        assert!(AspectWeights::from_sliders(f32::NAN, [0.0; 4]).is_err());
    }

    #[test]
    fn rejects_bad_weights() {
        for bad in [
            "",
            "pitch",
            "pitch=x",
            "colour=1",
            "pitch=-1",
            "pitch=nan",
            "pitch=inf",
            "pitch=0",
        ] {
            assert!(
                matches!(
                    bad.parse::<AspectWeights>(),
                    Err(Error::InvalidQuery { .. })
                ),
                "{bad:?}"
            );
        }
        assert!(AspectWeights::overall(f32::NAN).validate().is_err());
        assert!(AspectWeights::default().validate().is_ok());
    }
}
