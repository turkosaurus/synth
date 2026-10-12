use std::io::Stdout;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use crossterm::event;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Gauge, Paragraph, RenderDirection, Sparkline, SparklineBar};
use ratatui::{Frame, Terminal};

use crate::audio::{self, Event, Player, Snapshot, Stats};

use crate::input::keyboard::{Gating, Keyboard, Outcome};
use crate::script;

use super::meter::{self, Activity, Meter};

const QUEUE: usize = 256;
const POLL: Duration = Duration::from_millis(16);

pub fn run(path: Option<&std::path::Path>, gating: Gating) -> anyhow::Result<()> {
    let output = audio::open()?;
    let patch = match path {
        Some(path) => script::load(path, output.sample_rate)?,
        None => script::load_default(output.sample_rate)?,
    };

    // the view reads the same baked table the player does, and the player
    // takes ownership of it. 128 f32 copied once at startup.
    let freqs = patch.freqs.clone();

    // the producer stays here; the consumer moves into the audio callback
    let (tx, rx) = rtrb::RingBuffer::<Event>::new(QUEUE);
    let player = Player::live(
        patch.timeline.instruments,
        patch.freqs,
        rx,
        output.sample_rate as f32,
    );

    let stats = Arc::new(Stats::default());
    let _stream = audio::stream(&output, player, Arc::clone(&stats))?;

    let mut keyboard = Keyboard::new(patch.keymap, gating, tx);

    // built before enter(), so a failure here needs no teardown.
    // backend goes straight over stdout rather than through ratatui::init()
    let backend = CrosstermBackend::new(std::io::stdout());
    let mut terminal = Terminal::new(backend).context("building terminal")?;

    crate::terminal::enter(gating)?;
    let result = update(&mut terminal, &mut keyboard, &stats, &freqs, gating);
    crate::terminal::leave(gating);
    result
}

fn update(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    keyboard: &mut Keyboard,
    stats: &Stats,
    freqs: &[f32],
    gating: Gating,
) -> anyhow::Result<()> {
    // ballistics are the view's, not the device's: the callback has no
    // business knowing the frame rate a bar falls at
    let mut peak = Meter::default();
    let mut activity = Activity::default();
    let mut last = Instant::now();

    loop {
        // one snapshot() per frame gauges reset,
        // so widgets must share one capture
        let snap = stats.snapshot();

        let now = Instant::now();
        let dt = now - last;
        last = now;
        peak.update(meter::scale(snap.peak), dt);
        activity.update(meter::scale(snap.peak), snap.clips);

        terminal.draw(|frame| draw(frame, keyboard, &snap, &peak, &activity, freqs, gating))?;

        if event::poll(POLL)? {
            // drain immediately
            while event::poll(Duration::ZERO)? {
                if let event::Event::Key(key) = event::read()? {
                    if keyboard.handle(key) == Outcome::Quit {
                        return Ok(());
                    }
                }
            }
        }

        // ends a Fixed-mode note whose time is up; a no-op when Held
        keyboard.tick();
    }
}

fn draw(
    frame: &mut Frame,
    keyboard: &Keyboard,
    snap: &Snapshot,
    peak: &Meter,
    activity: &Activity,
    freqs: &[f32],
    gating: Gating,
) {
    let sounding = match keyboard.sounding() {
        // tuning.lua defines frequency
        Some(note) => {
            let hz = freqs.get(note as usize).copied().unwrap_or(0.0);
            format!("{} ({hz:.1} Hz)", name(note))
        }
        None => "—".to_string(),
    };
    let mode = match gating {
        Gating::Held => "held".to_string(),
        Gating::Fixed(length) => format!("fixed {}ms", length.as_millis()),
    };

    let input_lines = vec![
        Line::from(format!("note    {sounding}")),
        Line::from(format!(
            "char     {}      octave  {:+}",
            keyboard.last_key().unwrap_or(' '),
            keyboard.octave()
        )),
        Line::from(format!("gating  {mode}")),
    ];

    let footer = vec![Line::from("ctrl-c/d  quit")];

    let outer = Block::bordered().title(" synth ");
    let inner = outer.inner(frame.area());
    // the footer is unlabelled and borderless, so the two sections above it
    // give up a row rather than it carrying a box of its own
    let [main, footer_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(inner);
    let [input_area, stats_area] =
        Layout::horizontal([Constraint::Length(15), Constraint::Min(0)]).areas(main);

    frame.render_widget(outer, frame.area());
    frame.render_widget(
        Paragraph::new(input_lines).block(Block::bordered().title(" input ")),
        input_area,
    );
    // bound like `outer` so its inner area can be split: the gauge and the
    // text sit inside one border rather than carrying one each
    let stats_block = Block::bordered().title(" stats ");
    let stats_inner = stats_block.inner(stats_area);
    frame.render_widget(stats_block, stats_area);

    // Flex::Start is the default, so whatever is left under the last row
    // simply stays empty and needs no filler constraint
    let [load_area, peak_area, clip_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(2),
    ])
    .areas(stats_inner);

    // load's full scale is the deadline itself, so a healthy stream reads
    // nearly empty; that space is the headroom polyphony gets to spend.
    frame.render_widget(
        bar(
            format!("cpu {:>6.1}%", snap.load * 100.0),
            snap.load,
            0.5,
            0.9,
        ),
        load_area,
    );
    // peak is logarithmic, so the thresholds are dB: yellow from -12,
    // red from about -2, where clipping is in reach
    frame.render_widget(
        bar(
            format!("gain {} dB", meter::db_label(peak.level())),
            peak.level(),
            0.8,
            0.99,
        ),
        peak_area,
    );
    frame.render_widget(
        Sparkline::default()
            // height is the level, colour is whether that frame clipped:
            // one encoding, because clipping is a property of a loud frame
            .data(activity.history().map(|(level, clipped)| {
                let colour = if clipped { Color::Red } else { Color::Green };
                SparklineBar::from(level).style(Style::new().fg(colour))
            }))
            // absolute heights, so a quiet passage is not drawn as tall as
            // a loud one by scaling to whatever is in the window
            .max(meter::STEPS)
            // index 0 is the newest sample, so it belongs at the right
            // edge; this also drops the oldest when the row is narrow
            .direction(RenderDirection::RightToLeft)
            // a zero bar is a space, so without a background a silent
            // stream draws nothing at all. bg paints every rendered cell,
            // which turns the row into a track the bars sit in.
            .style(Style::new().bg(Color::DarkGray)),
        clip_area,
    );

    // drawn after the spark, so these land on top of it. the left end is
    // the oldest history, which is the cheapest part of the row to cover:
    // Sparkline has no read-through-the-label trick, so the cells under
    // text are overwritten rather than composited.
    let [peak_row, clip_row] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(clip_area);
    let over = Style::new().bg(Color::DarkGray);
    frame.render_widget(
        // the newest bar, not the ballistic meter, so the number describes
        // the bar it sits next to rather than the gauge above
        Line::from(format!(
            " peak {} dB ",
            meter::db_label(meter::scale(snap.peak))
        ))
        .style(over),
        peak_row,
    );
    frame.render_widget(
        Line::from(format!(" clips {} ", activity.clips())).style(over),
        clip_row,
    );

    frame.render_widget(Paragraph::new(footer), footer_area);
}

/// One labelled bar, turning yellow at `warn` and red at `danger`.
///
/// `ratio` panics outside 0.0..=1.0 and load is not bounded above, so the
/// bar is clamped while the caller's label keeps the true figure: an
/// excursion reads past full instead of disappearing.
fn bar(label: String, value: f32, warn: f32, danger: f32) -> Gauge<'static> {
    let colour = match value {
        value if value < warn => Color::Green,
        value if value < danger => Color::Yellow,
        _ => Color::Red,
    };
    Gauge::default()
        .label(label)
        // fg fills, bg is the empty track; one style covers both
        .gauge_style(Style::new().fg(colour).bg(Color::DarkGray))
        // eighth-blocks, so a few percent is still a visible sliver
        // rather than rounding away to an empty bar
        .use_unicode(true)
        .ratio(value.clamp(0.0, 1.0) as f64)
}

fn name(note: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!("{}{}", NAMES[(note % 12) as usize], note as i32 / 12 - 1)
}
