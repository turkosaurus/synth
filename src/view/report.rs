//! Printed reports: patch, config, environment.

use crate::audio::Snapshot;
use crate::input::keyboard::Gating;
use crate::spec::{Config, Patch};

use super::table;

pub fn environment(gating: Gating) {
    let term = std::env::var("TERM").unwrap_or_default();
    let in_tmux = std::env::var("TMUX").is_ok();

    let (release, mode) = match gating {
        Gating::Held => ("yes".to_string(), "held notes".to_string()),
        // the length is the keyboard's gate, which belongs to a patch
        Gating::Fixed => ("no".to_string(), "fixed length (keyboard gate)".to_string()),
    };

    table::pairs(
        "environment",
        &[
            ("terminal", term),
            (
                "multiplexer",
                if in_tmux { "tmux" } else { "none" }.to_string(),
            ),
            ("key release", release),
            ("gating", mode),
        ],
        false,
    );

    if in_tmux {
        println!("  tmux blocks the kitty keyboard protocol; re-run outside tmux");
    }
    // the cost of no key release, said where it can be fixed rather than
    // left to sound like a fault in the engine
    if let Gating::Fixed = gating {
        println!(
            "  no key release: every note lasts the keyboard's gate, and a held key \
             sounds once, then again at the OS repeat delay; a terminal with the \
             kitty keyboard protocol reports release"
        );
    }
}

/// Where `config.lua` is looked for, and the settings in effect.
pub fn config(path: Option<&std::path::Path>, config: &Config) {
    let file = match path {
        Some(path) if path.exists() => path.display().to_string(),
        Some(path) => format!("{} (none; defaults)", path.display()),
        None => "no XDG_CONFIG_HOME or HOME (defaults)".to_string(),
    };
    let buffer = match config.buffer_frames {
        Some(frames) => format!("{frames} frames"),
        None => "platform default".to_string(),
    };
    let ms = |d: std::time::Duration| format!("{} ms", d.as_millis());
    let tui = &config.tui;
    table::pairs(
        "config",
        &[
            ("file", file),
            ("buffer", buffer),
            ("queue", format!("{} events", config.queue)),
            (
                "custom_dir",
                match &config.custom_dir {
                    Some(dir) => dir.display().to_string(),
                    None => "none (no HOME)".to_string(),
                },
            ),
            ("tui poll", ms(tui.poll)),
            ("meter hold", ms(tui.meter_hold)),
            ("meter fall", ms(tui.meter_fall)),
            ("meter floor", format!("{} dB", tui.meter_floor_db)),
        ],
        false,
    );
}

/// What `synth custom` did to the built-in copies: one line for each file
/// it changed or kept, nothing for those already current.
/// With `asking`, an edited copy behind its built-in is left to the prompt
/// that follows; without, the line says how to be asked.
pub fn builtins_written(wrote: &[(&str, crate::script::builtins::Wrote)], asking: bool) {
    use crate::script::builtins::Wrote;
    for (name, outcome) in wrote {
        match outcome {
            Wrote::Written => println!("wrote {name}"),
            Wrote::Refreshed => println!("refreshed {name} to the current built-in"),
            Wrote::Kept => println!("kept {name}: edited"),
            Wrote::KeptBehind if asking => {}
            Wrote::KeptBehind => println!(
                "kept {name}: edited, and the built-in has changed since; \
                 run synth custom in a terminal to choose"
            ),
            Wrote::Current => {}
        }
    }
}

/// Each built-in and whether its copy is edited.
pub fn builtins(states: &[(&str, crate::script::builtins::State)]) {
    use crate::script::builtins::State;
    let rows: Vec<(&str, String)> = states
        .iter()
        .map(|(name, state)| {
            let value = match state {
                State::BuiltIn => "built in".to_string(),
                State::Edited(path) => format!("edited: {}", path.display()),
                State::EditedBehind(path) => {
                    format!("edited, built-in newer: {}", path.display())
                }
            };
            (*name, value)
        })
        .collect();
    table::pairs("built-in copies", &rows, false);
}

/// Everything `check` reports about a loaded patch. Computes nothing:
/// every figure here is already a field on [`Patch`] or its reports.
pub fn patch(path: &std::path::Path, patch: &Patch, sample_rate: u32) {
    let ms = |samples: usize| format!("{:.1} ms", samples as f64 * 1000.0 / sample_rate as f64);

    if patch.bpm > 0.0 {
        let mut rows = vec![
            ("bpm", format!("{:.0}", patch.bpm)),
            ("division", format!("{:.0}", patch.division)),
            ("step", format!("{:.1} ms", patch.step_ms)),
        ];
        if patch.patterns > 0 {
            rows.push(("patterns", table::commas(patch.patterns)));
            rows.push((
                "chain",
                format!("{} × {}", table::commas(patch.chain), patch.loops),
            ));
        }
        rows.push(("steps", table::commas(patch.steps)));
        rows.push(("events", table::commas(patch.timeline.events.len())));
        rows.push((
            "length",
            format!(
                "{:.1} s",
                patch.timeline.total_samples as f64 / sample_rate as f64
            ),
        ));

        table::pairs(&format!("sequence  ({})", path.display()), &rows, true);
        println!();
    }

    for (name, report) in &patch.reports {
        table::pairs(
            &format!("instrument {name}  (rate {sample_rate})"),
            &[
                ("frames", table::commas(report.frames)),
                ("peak", format!("{:.3}", report.peak)),
                ("dc", format!("{:+.4}", report.dc)),
                ("seam", format!("{:.4}", report.seam)),
                ("clamped", table::commas(report.clamped)),
            ],
            true,
        );
        println!();

        table::columns(
            "envelope",
            &["segment", "samples", "duration"],
            vec![
                vec![
                    "attack".into(),
                    table::commas(report.attack),
                    ms(report.attack),
                ],
                vec![
                    "decay".into(),
                    table::commas(report.decay),
                    ms(report.decay),
                ],
                vec![
                    "sustain".into(),
                    "—".into(),
                    format!("{:.3}", report.sustain_level),
                ],
                vec![
                    "release".into(),
                    table::commas(report.release),
                    ms(report.release),
                ],
            ],
        );

        for diagnostic in &report.diagnostics {
            println!("  {}  {name}: {diagnostic}", diagnostic.severity().label());
        }
        println!();
    }

    if let Some(report) = &patch.filter_report {
        let mut rows = vec![
            ("kind", report.kind.to_string()),
            ("cutoff", format!("{} Hz", report.cutoff)),
        ];
        if let Some(resonance) = report.resonance {
            rows.push(("resonance", format!("{resonance:.2}")));
        }
        if let Some(gain) = report.gain {
            rows.push(("gain", format!("{gain:+.1} dB")));
        }
        table::pairs("filter  (summed output)", &rows, true);
        for diagnostic in &report.diagnostics {
            println!("  {}  filter: {diagnostic}", diagnostic.severity().label());
        }
        println!();
    }

    for diagnostic in patch.tuning_diagnostics.iter().chain(&patch.printed) {
        println!("  {}  {diagnostic}", diagnostic.severity().label());
    }
    if !(patch.tuning_diagnostics.is_empty() && patch.printed.is_empty()) {
        println!();
    }

    // where every value came from: the path is what a reload will match on,
    // and the line is where to look when a value is wrong
    let rows = patch
        .structure
        .iter()
        .map(|node| {
            let path = if node.path.is_empty() {
                "(root)".to_string()
            } else {
                node.path.clone()
            };
            vec![path, node.kind.clone(), node.at.clone()]
        })
        .collect();
    table::columns("structure", &["path", "kind", "made at"], rows);
    println!();

    if patch.clean() {
        println!("  ok");
    }
}

/// One line of stream telemetry. Returns rather than prints, because the tui
/// needs raw-mode line endings and `play` does not; both embed this in their
/// own layout.
pub fn stats(snap: &Snapshot) -> String {
    let Snapshot {
        frames,
        clipped,
        non_finite,
        overruns,
        errors,
        dropped,
        peak,
        budget_us,
        callback_us_max,
        load,
    } = *snap;
    let pct = load * 100.0;
    format!(
        "frames={frames} clipped={clipped} non_finite={non_finite} peak={peak:.3} \
         callback_max={callback_us_max}us budget={budget_us}us ({pct:.1}%) \
         overruns={overruns} errors={errors} dropped={dropped}"
    )
}
