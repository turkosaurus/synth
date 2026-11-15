use crate::audio::Snapshot;
use crate::input::keyboard::Gating;
use crate::spec::Patch;

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

        for warning in &report.warnings {
            println!("  warn  {name}: {warning}");
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
        for warning in &report.warnings {
            println!("  warn  filter: {warning}");
        }
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
        clips,
        overruns,
        errors,
        peak,
        budget_us,
        callback_us_max,
        load,
    } = *snap;
    let pct = load * 100.0;
    format!(
        "frames={frames} clips={clips} peak={peak:.3} \
         callback_max={callback_us_max}us budget={budget_us}us ({pct:.1}%) \
         overruns={overruns} errors={errors}"
    )
}
