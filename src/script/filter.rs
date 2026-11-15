//! The top-level `filter` field: a table from `lua/filter.lua`, read into
//! plain `Filter` data for the audio side.

use std::f32::consts::SQRT_2;

use crate::audio::{Filter, Kind, MIN_DAMPING};

use super::bake::{expect_only, spec_entry, spec_error, spec_table};
use super::field;

const WHAT: &str = "filter";

pub(super) struct FilterSpec(pub(super) Filter);

impl mlua::FromLua for FilterSpec {
    fn from_lua(value: mlua::Value, _: &mlua::Lua) -> mlua::Result<Self> {
        let table = spec_table(value, WHAT)?;
        let name: String = spec_entry(&table, WHAT, field::KIND, "filter kind")?;
        let (kind, resonance, gain) = match name.as_str() {
            "lowpass" => (Kind::Lowpass, true, false),
            "highpass" => (Kind::Highpass, true, false),
            "bandpass" => (Kind::Bandpass, true, false),
            "notch" => (Kind::Notch, true, false),
            "bell" => (Kind::Bell, true, true),
            "low_shelf" => (Kind::LowShelf, false, true),
            "high_shelf" => (Kind::HighShelf, false, true),
            other => {
                return Err(spec_error(
                    "table",
                    WHAT,
                    format!("unknown kind `{other}`; build it with synth.filter"),
                ))
            }
        };

        let mut allowed = vec![field::KIND, field::CUTOFF];
        if resonance {
            allowed.push(field::RESONANCE);
        }
        if gain {
            allowed.push(field::GAIN);
        }
        expect_only(&table, WHAT, &allowed)?;

        let cutoff: f64 = spec_entry(&table, WHAT, field::CUTOFF, "number")?;
        if !(cutoff > 0.0 && cutoff.is_finite()) {
            return Err(invalid(format!("`cutoff` must be positive Hz, got {cutoff}")));
        }

        // 0 is flat (Butterworth); 1 rings as hard as stays stable
        let damping = if resonance {
            let r: f64 = spec_entry(&table, WHAT, field::RESONANCE, "number")?;
            if !(0.0..=1.0).contains(&r) {
                return Err(invalid(format!("`resonance` must be 0 to 1, got {r}")));
            }
            if r == 1.0 {
                MIN_DAMPING
            } else {
                SQRT_2 - r as f32 * (SQRT_2 - MIN_DAMPING)
            }
        } else {
            SQRT_2
        };

        // the square root of the shelf amplitude, computed here because
        // the audio side has no `sqrt`
        let root = if gain {
            let db: f64 = spec_entry(&table, WHAT, field::GAIN, "number")?;
            if !db.is_finite() {
                return Err(invalid(format!("`gain` must be finite dB, got {db}")));
            }
            10f64.powf(db / 80.0) as f32
        } else {
            1.0
        };

        Ok(Self(Filter {
            kind,
            cutoff: cutoff as f32,
            damping,
            root,
        }))
    }
}

fn invalid(message: String) -> mlua::Error {
    spec_error("table", WHAT, message)
}
