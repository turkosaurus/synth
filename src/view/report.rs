use crate::input::keyboard::Gating;
use crate::script::Patch;

use super::table;

pub fn environment(gating: Gating) {
    let term = std::env::var("TERM").unwrap_or_default();
    let in_tmux = std::env::var("TMUX").is_ok();

    let (release, mode) = match gating {
        Gating::Held => ("yes".to_string(), "held notes".to_string()),
        Gating::Fixed(length) => ("no".to_string(), format!("fixed {} ms", length.as_millis())),
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
            &format!("voice {name}  (rate {sample_rate})"),
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

    if patch.clean() {
        println!("  ok");
    }
}
