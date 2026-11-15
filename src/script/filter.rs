//! The top-level `filter` field: a table from `lua/filter.lua`, read into
//! plain `Filter` data for the audio side, plus a report of what was
//! written for `check`.

use std::f32::consts::SQRT_2;

use crate::audio::{Filter, Kind, MAX_CUTOFF, MIN_CUTOFF, MIN_DAMPING};

use super::bake::{expect_only, spec_entry, spec_error, spec_table};
use super::field;

const WHAT: &str = "filter";

pub(super) struct FilterSpec {
    pub(super) filter: Filter,
    name: &'static str,
    // as written in Lua, for the report
    cutoff: f64,
    resonance: Option<f64>,
    gain: Option<f64>,
}

/// What `check` shows about a patch's filter, in the units Lua used.
#[derive(Debug)]
pub struct FilterReport {
    /// the constructor's name, e.g. `low_shelf`
    pub kind: &'static str,
    /// Hz, as written; the audio side may clamp it, which `warnings` says
    pub cutoff: f64,
    /// 0 to 1; `None` on shelves, which have none
    pub resonance: Option<f64>,
    /// dB; `None` on kinds without gain
    pub gain: Option<f64>,
    pub warnings: Vec<String>,
}

impl FilterSpec {
    /// The sample rate decides where the cutoff clamps, so a cutoff that is
    /// fine at 96 kHz can warn at 48 kHz.
    pub(super) fn report(&self, sample_rate: u32) -> FilterReport {
        let ceiling = f64::from(MAX_CUTOFF) * f64::from(sample_rate);
        let floor = f64::from(MIN_CUTOFF);
        let mut warnings = Vec::new();
        if self.cutoff > ceiling {
            warnings.push(format!(
                "cutoff {} Hz is above the {ceiling:.0} Hz ceiling at {sample_rate} Hz \
                 and plays as {ceiling:.0} Hz",
                self.cutoff
            ));
        } else if self.cutoff < floor {
            warnings.push(format!(
                "cutoff {} Hz is below the {floor} Hz floor and plays as {floor} Hz",
                self.cutoff
            ));
        }
        FilterReport {
            kind: self.name,
            cutoff: self.cutoff,
            resonance: self.resonance,
            gain: self.gain,
            warnings,
        }
    }
}

impl mlua::FromLua for FilterSpec {
    fn from_lua(value: mlua::Value, _: &mlua::Lua) -> mlua::Result<Self> {
        let table = spec_table(value, WHAT)?;
        let tag: String = spec_entry(&table, WHAT, field::KIND, "filter kind")?;
        let (kind, name, has_resonance, has_gain) = match tag.as_str() {
            "lowpass" => (Kind::Lowpass, "lowpass", true, false),
            "highpass" => (Kind::Highpass, "highpass", true, false),
            "bandpass" => (Kind::Bandpass, "bandpass", true, false),
            "notch" => (Kind::Notch, "notch", true, false),
            "bell" => (Kind::Bell, "bell", true, true),
            "low_shelf" => (Kind::LowShelf, "low_shelf", false, true),
            "high_shelf" => (Kind::HighShelf, "high_shelf", false, true),
            other => {
                return Err(invalid(format!(
                    "unknown kind `{other}`; build it with synth.filter"
                )))
            }
        };

        let mut allowed = vec![field::KIND, field::CUTOFF];
        if has_resonance {
            allowed.push(field::RESONANCE);
        }
        if has_gain {
            allowed.push(field::GAIN);
        }
        expect_only(&table, WHAT, &allowed)?;

        let cutoff: f64 = spec_entry(&table, WHAT, field::CUTOFF, "number")?;
        if !(cutoff > 0.0 && cutoff.is_finite()) {
            return Err(invalid(format!("`cutoff` must be positive Hz, got {cutoff}")));
        }

        let resonance = if has_resonance {
            let r: f64 = spec_entry(&table, WHAT, field::RESONANCE, "number")?;
            if !(0.0..=1.0).contains(&r) {
                return Err(invalid(format!("`resonance` must be 0 to 1, got {r}")));
            }
            Some(r)
        } else {
            None
        };

        // 0 is flat (Butterworth); 1 rings as hard as stays stable
        let damping = match resonance {
            Some(1.0) => MIN_DAMPING,
            Some(r) => SQRT_2 - r as f32 * (SQRT_2 - MIN_DAMPING),
            None => SQRT_2,
        };

        let gain = if has_gain {
            let db: f64 = spec_entry(&table, WHAT, field::GAIN, "number")?;
            if !db.is_finite() {
                return Err(invalid(format!("`gain` must be finite dB, got {db}")));
            }
            Some(db)
        } else {
            None
        };

        // the square root of the shelf amplitude, computed here because
        // the audio side has no `sqrt`
        let root = gain.map_or(1.0, |db| 10f64.powf(db / 80.0) as f32);

        Ok(Self {
            filter: Filter {
                kind,
                cutoff: cutoff as f32,
                damping,
                root,
            },
            name,
            cutoff,
            resonance,
            gain,
        })
    }
}

fn invalid(message: String) -> mlua::Error {
    spec_error("table", WHAT, message)
}
