//! Drives a `Session` and draws it.

use std::io::Stdout;
use std::time::{Duration, Instant};

use anyhow::Context;
use crossterm::event::{self, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Flex, Layout, Rect, Spacing};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::merge::MergeStrategy;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Cell, Clear, Gauge, Paragraph, RenderDirection, Row, Sparkline, SparklineBar, Table,
};
use ratatui::{Frame, Terminal};

use crate::audio::Snapshot;
use crate::input::keyboard::{Gating, Keyboard, Kind, Stroke};
use crate::session::{Latency, Msg, Session, Spread, Window};
use crate::spec::{Budget, Freqs, Key, TuiConfig};

use super::meter::{self, Activity, Meter};

const POPUP_FILL: Color = Color::Yellow;
const POPUP_INK: Color = Color::Black;

const VALUE_WIDTH: u16 = 16;
const LABEL_WIDTH: u16 = 6;
const INPUT_WIDTH: u16 = LABEL_WIDTH + COLUMN_GAP + VALUE_WIDTH + 2;
const COLUMN_GAP: u16 = 2;
const SPARK_ROWS: u16 = 2;
const METER_ROWS: u16 = 3 + SPARK_ROWS;
/// the header and four stages
const LATENCY_ROWS: u16 = 5;
const STATS_ROWS: u16 = METER_ROWS + LATENCY_ROWS;
const STAGE_WIDTH: u16 = 7;
const NUMBER_WIDTH: u16 = 7;
/// Below this the bar keeps only the p99 column; below the next, only
/// numbers.
const LATENCY_FULL: u16 = 60;
const LATENCY_BAR: u16 = 40;

/// Draw `session` and feed it keys until the player quits.
pub fn run(session: &mut Session, config: &TuiConfig) -> anyhow::Result<()> {
    let gating = session.model().keyboard.gating();

    // built before enter(), so a failure here needs no teardown.
    // backend goes straight over stdout rather than through ratatui::init()
    let backend = CrosstermBackend::new(std::io::stdout());
    let mut terminal = Terminal::new(backend).context("building terminal")?;

    // restores the terminal on every way out of here, a panic included
    let _restore = crate::terminal::enter(gating)?;
    drive(&mut terminal, session, gating, config)
}

fn drive(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    session: &mut Session,
    gating: Gating,
    config: &TuiConfig,
) -> anyhow::Result<()> {
    // ballistics are the view's, not the device's: the callback has no
    // business knowing the frame rate a bar falls at
    let mut peak = Meter::new(config.meter_hold, config.meter_fall);
    let mut activity = Activity::default();
    let mut last = Instant::now();

    loop {
        // SIGTERM and SIGHUP only set a flag; leaving here restores the
        // terminal through the same path as the quit key
        if crate::terminal::interrupted() {
            return Ok(());
        }

        // one snapshot() per frame gauges reset,
        // so widgets must share one capture
        let snap = session.stats.snapshot();

        let now = Instant::now();
        let dt = now - last;
        last = now;
        peak.update(meter::scale(snap.peak, config.meter_floor_db), dt);
        activity.update(meter::scale(snap.peak, config.meter_floor_db), snap.clipped);

        let model = session.model();
        // spread sorts each window, so it is taken before the draw it
        // would otherwise be counted in
        let latency = stages(&model.latency, config);
        let fps = model.latency.view.per_second();
        let view = View {
            keyboard: &model.keyboard,
            snap: &snap,
            peak: &peak,
            activity: &activity,
            freqs: &model.freqs,
            gating,
            help: model.help,
            floor_db: config.meter_floor_db,
            latency: &latency,
            fps,
            requested: model.requested,
            config,
        };
        terminal.draw(|frame| draw(frame, &view))?;
        let _ = session.update(Msg::Drawn(Instant::now()));

        if event::poll(config.poll)? {
            // drain immediately
            while event::poll(Duration::ZERO)? {
                let event::Event::Key(key) = event::read()? else {
                    continue;
                };
                let read = Instant::now();
                let Some(stroke) = stroke(key) else {
                    continue;
                };
                if session.update(Msg::Key(stroke, read)).is_break() {
                    return Ok(());
                }
            }
        }

        if session.update(Msg::Tick).is_break() {
            return Ok(());
        }
    }
}

/// The terminal's key as the keyboard reads it; `None` for a key no binding
/// can name, which no binding could have matched.
fn stroke(event: KeyEvent) -> Option<Stroke> {
    Some(Stroke {
        key: key(event.code)?,
        kind: match event.kind {
            KeyEventKind::Press => Kind::Press,
            KeyEventKind::Release => Kind::Release,
            KeyEventKind::Repeat => Kind::Repeat,
        },
        ctrl: event.modifiers.contains(KeyModifiers::CONTROL),
    })
}

fn key(code: KeyCode) -> Option<Key> {
    Some(match code {
        KeyCode::Char(ch) => Key::Char(ch),
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Delete => Key::Delete,
        KeyCode::Enter => Key::Enter,
        KeyCode::Tab => Key::Tab,
        KeyCode::Esc => Key::Esc,
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        _ => return None,
    })
}

/// A binding the way the terminal names it, for the help overlay.
fn code(key: Key) -> KeyCode {
    match key {
        Key::Char(ch) => KeyCode::Char(ch),
        Key::Backspace => KeyCode::Backspace,
        Key::Delete => KeyCode::Delete,
        Key::Enter => KeyCode::Enter,
        Key::Tab => KeyCode::Tab,
        Key::Esc => KeyCode::Esc,
        Key::Up => KeyCode::Up,
        Key::Down => KeyCode::Down,
        Key::Left => KeyCode::Left,
        Key::Right => KeyCode::Right,
        Key::Home => KeyCode::Home,
        Key::End => KeyCode::End,
        Key::PageUp => KeyCode::PageUp,
        Key::PageDown => KeyCode::PageDown,
    }
}

/// Everything one frame is drawn from, gathered once per iteration.
///
/// Grouped rather than passed positionally: the list had reached the point
/// where the order of eight arguments was the only thing telling them apart,
/// and whatever a second screen needs will land here too.
#[derive(Clone, Copy)]
struct View<'a> {
    keyboard: &'a Keyboard,
    snap: &'a Snapshot,
    peak: &'a Meter,
    activity: &'a Activity,
    freqs: &'a Freqs,
    gating: Gating,
    help: bool,
    /// the meter's floor, from `config.lua`
    floor_db: f32,
    latency: &'a [Stage],
    /// frames drawn in the last second
    fps: usize,
    /// frames per callback asked of the device; `None` left it to the
    /// platform
    requested: Option<u32>,
    config: &'a TuiConfig,
}

/// One latency row: a stage, its spread, and its budget.
struct Stage {
    name: &'static str,
    spread: Option<Spread<Duration>>,
    budget: Budget,
}

/// The stages in signal order, then their per-key total.
fn stages(latency: &Latency, config: &TuiConfig) -> Vec<Stage> {
    let stage = |name, window: &Window<Duration>, budget| Stage {
        name,
        spread: window.spread(),
        budget,
    };
    vec![
        stage("input", &latency.input, config.input_budget),
        stage("render", &latency.render, config.render_budget),
        stage("output", &latency.output, config.output_budget),
        stage("total", &latency.total, config.total_budget),
    ]
}

/// The buffer the device gave, and whether it differs from the one asked
/// for. `got` is 0 before the first callback.
fn buffer(requested: Option<u32>, got: u32) -> (String, bool) {
    let shown = match got {
        0 => "—".to_string(),
        frames => frames.to_string(),
    };
    match requested {
        None => (format!("buffer {shown} (default)"), false),
        Some(asked) if got == 0 || got == asked => (format!("buffer {asked}"), false),
        Some(asked) => (format!("buffer {shown} (asked {asked})"), true),
    }
}

fn fps_style(fps: usize, config: &TuiConfig) -> Style {
    match fps {
        fps if fps < config.fps_danger as usize => Style::new().fg(Color::Red),
        fps if fps < config.fps_warn as usize => Style::new().fg(Color::Yellow),
        _ => Style::new(),
    }
}

/// The one status line: fault counters, the buffer, and the view's rate.
/// An unplugged device or a stalled callback otherwise looks like a quiet
/// synth, so each counter takes colour once it moves.
fn status(
    snap: &Snapshot,
    requested: Option<u32>,
    fps: usize,
    config: &TuiConfig,
) -> Line<'static> {
    let count = |name: &str, n: u64, colour: Color| {
        let style = match n {
            0 => Style::new(),
            _ => Style::new().fg(colour),
        };
        Span::styled(format!("{name} {n}  "), style)
    };
    let (buffer, differs) = buffer(requested, snap.buffer_frames);
    let buffer_style = match differs {
        true => Style::new().fg(Color::Yellow),
        false => Style::new(),
    };
    Line::from(vec![
        count("errors", snap.errors, Color::Red),
        count("overruns", snap.overruns, Color::Red),
        count("dropped", snap.dropped, Color::Yellow),
        Span::styled(buffer, buffer_style),
        Span::styled(format!("  view {fps} fps"), fps_style(fps, config)),
    ])
}

fn draw(frame: &mut Frame, view: &View) {
    let View {
        keyboard,
        snap,
        peak,
        activity,
        freqs,
        gating,
        help,
        floor_db,
        latency,
        fps,
        requested,
        config,
    } = *view;

    let footer = vec![Line::from("(ctrl-c) quit | (?) help")];

    // stats is sized to exactly what it draws: the taller of the meters and
    // the latency table, and its own borders. input takes what is left, so
    // the graphs stay put while the window grows.
    let [input_area, stats_area, footer_area] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(STATS_ROWS + 2),
        Constraint::Length(3),
    ])
    .spacing(Spacing::Overlap(1))
    .areas(frame.area());

    // input is sized to its table; the section beside it takes the rest, so
    // widening the window grows that one and leaves the table alone
    let [input_area, scope_area] =
        Layout::horizontal([Constraint::Length(INPUT_WIDTH), Constraint::Min(0)])
            .spacing(Spacing::Overlap(1))
            .areas(input_area);

    frame.render_widget(
        Paragraph::new("TODO")
            .centered()
            .style(Style::new().fg(Color::DarkGray))
            .block(pane().title(" scope ")),
        scope_area,
    );

    let input_block = pane().title(" input ");
    let input_inner = input_block.inner(input_area);
    frame.render_widget(input_block, input_area);
    frame.render_widget(state(keyboard, freqs, gating), input_inner);
    // bound so its inner area can be split: the gauges and the sparkline sit
    // inside one border rather than carrying one each
    let stats_block = pane().title(" stats ");
    let stats_inner = stats_block.inner(stats_area);
    frame.render_widget(stats_block, stats_area);

    // latency, then graph, then gain, then cpu, then status at the very
    // bottom: region order is what stacks them, not the order the widgets
    // are rendered in
    let [latency_area, clip_area, peak_area, load_area, status_area] = Layout::vertical([
        Constraint::Length(LATENCY_ROWS),
        Constraint::Length(SPARK_ROWS),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(stats_inner);
    let scale = Scale {
        floor_ms: config.latency_floor_ms,
        ceil_ms: config.latency_ceil_ms,
    };
    latency_rows(frame, latency_area, latency, scale);
    frame.render_widget(status(snap, requested, fps, config), status_area);

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

    // the app name rides the frame's bottom edge, since the panes tile the
    // whole area and there is no wrapping block left to carry it
    frame.render_widget(
        Paragraph::new(footer).right_aligned().block(pane()),
        footer_area,
    );
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
    // peak is logarithmic, so the thresholds are dB, placed on whatever
    // floor the scale has
    let at = |db: f32| meter::scale(10f32.powf(db / 20.0), floor_db);
    frame.render_widget(
        bar(
            format!(
                "gain {} dB  hold {} dB",
                meter::db_label(peak.level(), floor_db),
                meter::db_label(peak.hold(), floor_db),
            ),
            peak.level(),
            at(-12.0),
            at(-0.6),
        ),
        peak_area,
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
            meter::db_label(meter::scale(snap.peak, floor_db), floor_db)
        ))
        .style(over),
        peak_row,
    );
    frame.render_widget(
        Line::from(format!(" clipped {} ", activity.clipped())).style(over),
        clip_row,
    );

    // last, so it lands on top of everything already drawn
    if help {
        overlay(frame, keyboard);
    }
}

/// A bordered section.
///
/// `Spacing::Overlap(1)` makes neighbouring areas share the cell their
/// borders land on, and `MergeStrategy::Exact` resolves whatever lands there
/// into the right junction. Every pane keeps all four sides, so the panes
/// tile the whole frame and their outer edges *are* the frame: there is no
/// wrapping block, whose title would collide with input's on the shared top
/// row. Paired here so a new pane cannot forget the strategy.
fn pane() -> Block<'static> {
    Block::bordered().merge_borders(MergeStrategy::Exact)
}

/// The keymap, over the top of whatever is behind it.
fn overlay(frame: &mut Frame, keyboard: &Keyboard) {
    let [area] = Layout::horizontal([Constraint::Length(34)])
        .flex(Flex::Center)
        .areas(frame.area());
    // five bindings plus the two border rows
    let [area] = Layout::vertical([Constraint::Length(7)])
        .flex(Flex::Center)
        .areas(area);

    shadow(frame, area);
    // without Clear the cells the table does not write would still show what
    // is underneath, since ratatui only diffs what a widget touches
    frame.render_widget(Clear, area);

    // Block::render paints its style across the whole area first, so one
    // style covers the fill, the border and the titles. The table sets no
    // colours of its own, and Cell::set_style only assigns fields that are
    // Some, so the rows inherit both.
    let block = pane()
        .style(Style::new().fg(POPUP_INK).bg(POPUP_FILL))
        .title(Line::from(" keymap ").style(Style::new().add_modifier(Modifier::BOLD)))
        .title_bottom(" (/) close ");
    frame.render_widget(bindings(keyboard).block(block), area);
}

/// Darken the band one cell down and right of `area`, keeping the symbols
/// already there so it reads as a shadow rather than a black rectangle.
fn shadow(frame: &mut Frame, area: Rect) {
    let dim = Style::new().fg(Color::DarkGray).bg(Color::Black);
    let buffer = frame.buffer_mut();
    for y in area.top() + 1..area.bottom() + 1 {
        for x in area.left() + 1..area.right() + 1 {
            // the rest of the offset box is about to be covered anyway
            if x < area.right() && y < area.bottom() {
                continue;
            }
            if let Some(cell) = buffer.cell_mut((x, y)) {
                cell.set_style(dim);
            }
        }
    }
}

/// Read the bindings off the resolved keymap, so this screen cannot drift
/// from what the keys actually do.
///
/// A table rather than padded strings: the two columns are measured from
/// the content, and `Flex::Center` sits the pair in the middle of whatever
/// width the popup has instead of pinning them left.
fn bindings(keyboard: &Keyboard) -> Table<'static> {
    let keymap = keyboard.keymap();
    let keys = |keys: &[Key]| {
        keys.iter()
            .map(|&key| code(key).to_string())
            .collect::<Vec<_>>()
            .join(" or ")
    };

    let rows = [
        ("octave up", keys(&keymap.octave_up)),
        ("octave down", keys(&keymap.octave_down)),
        ("silence", keys(&keymap.stop)),
        ("help", keys(&keymap.help)),
        ("quit", "ctrl-c or ctrl-d".to_string()),
    ];

    let width = |pick: fn(&(&str, String)) -> usize| rows.iter().map(pick).max().unwrap_or(0);
    let labels = width(|(label, _)| label.chars().count());
    let widest = width(|(_, keys)| keys.chars().count());

    Table::new(
        rows.iter()
            .map(|(label, keys)| Row::new([(*label).to_string(), keys.clone()])),
        [
            Constraint::Length(labels as u16),
            Constraint::Length(widest as u16),
        ],
    )
    .column_spacing(2)
    .flex(Flex::Center)
}

/// One labelled bar, turning yellow at `warn` and red at `danger`.
///
/// `ratio` panics outside 0.0..=1.0 and load is not bounded above, so the
/// bar is clamped while the caller's label keeps the true figure: an
/// excursion reads past full instead of disappearing.
fn bar(label: String, value: f32, warn: f32, danger: f32) -> Gauge<'static> {
    let colour = health(value, warn, danger);
    Gauge::default()
        .label(label)
        // fg fills, bg is the empty track; one style covers both
        .gauge_style(Style::new().fg(colour).bg(Color::DarkGray))
        // eighth-blocks, so a few percent is still a visible sliver
        // rather than rounding away to an empty bar
        .use_unicode(true)
        .ratio(value.clamp(0.0, 1.0) as f64)
}

/// Green, then yellow at `warn`, then red at `danger`.
fn health(value: f32, warn: f32, danger: f32) -> Color {
    match value {
        value if value < warn => Color::Green,
        value if value < danger => Color::Yellow,
        _ => Color::Red,
    }
}

/// Live input state as a centred table.
///
/// Colour carries meaning rather than decoration: the label column is dim so
/// the values read first, a sounding note is green, a shifted octave is cyan
/// because it is modal state that is easy to forget, and `fixed` gating is
/// yellow because it means the terminal will not report key release, so note
/// length is not the player's choice.
fn state(keyboard: &Keyboard, freqs: &Freqs, gating: Gating) -> Table<'static> {
    let dim = Style::new().fg(Color::DarkGray);

    let note = match keyboard.sounding() {
        // the patch's tuning defines frequency
        Some(note) => {
            let hz = freqs.hz(note).unwrap_or(0.0);
            (
                format!("{} ({hz:.1} Hz)", name(note)),
                Style::new().fg(Color::Green),
            )
        }
        None => ("—".to_string(), dim),
    };

    let (octave, at_start) = keyboard.octave();
    let (low, high) = keyboard.reach();
    let octave = (
        format!("{octave:+}  ({low:+}..{high:+})"),
        // dim at the start, so the row lights up only once it has moved
        if at_start {
            dim
        } else {
            Style::new().fg(Color::Cyan)
        },
    );

    let gating = match gating {
        Gating::Held => ("gated".to_string(), dim),
        Gating::Fixed => (
            format!("{}ms", keyboard.keymap().gate.as_millis()),
            Style::new().fg(Color::Yellow),
        ),
    };

    let char = match keyboard.last_key() {
        Some(ch) => (ch.to_string(), Style::new().fg(Color::Reset)),
        None => ("—".to_string(), dim),
    };

    let keymap = keyboard.keymap();
    let layout = match keymap.name.as_deref() {
        Some(name) => (name.to_string(), Style::new().fg(Color::Reset)),
        // a patch supplying its own notes without a name
        None => ("custom".to_string(), dim),
    };

    let rows = [
        ("note", note),
        ("char", char),
        ("octave", octave),
        ("gating", gating),
        ("layout", layout),
    ];
    // fixed rather than measured, so the table's width matches the pane's
    // and neither drifts when a row is added
    Table::new(
        rows.into_iter().map(|(label, (value, style))| {
            Row::new([Cell::from(label).style(dim), Cell::from(value).style(style)])
        }),
        [
            Constraint::Length(LABEL_WIDTH),
            Constraint::Length(VALUE_WIDTH),
        ],
    )
    .column_spacing(COLUMN_GAP)
}

/// A number column's header, and the value it shows.
type Column = (&'static str, fn(&Spread<Duration>) -> Duration);

/// Which number columns a latency row shows, and whether it has a bar,
/// for the width it has.
fn latency_columns(width: u16) -> (&'static [Column], bool) {
    const ALL: [Column; 3] = [("p50", |s| s.p50), ("p99", |s| s.p99), ("max", |s| s.max)];
    const P99: [Column; 1] = [("p99", |s| s.p99)];
    match width {
        width if width >= LATENCY_FULL => (&ALL, true),
        width if width >= LATENCY_BAR => (&P99, true),
        _ => (&ALL, false),
    }
}

/// The header, then one row per stage: label, numbers in ms, and a bar on
/// one log scale shared by every row, so bars compare across rows.
fn latency_rows(frame: &mut Frame, area: Rect, stages: &[Stage], scale: Scale) {
    let dim = Style::new().fg(Color::DarkGray);
    let (columns, has_bar) = latency_columns(area.width);
    let mut widths = vec![Constraint::Length(STAGE_WIDTH)];
    widths.extend(columns.iter().map(|_| Constraint::Length(NUMBER_WIDTH)));
    widths.push(Constraint::Min(0));
    let split = |row: Rect| Layout::horizontal(widths.clone()).spacing(1).split(row);
    let right = |text: String| Line::from(text).right_aligned();

    let mut rows = area.rows();
    let Some(header) = rows.next() else {
        return;
    };
    let cells = split(header);
    frame.render_widget(Line::from("latency").style(dim), cells[0]);
    for ((name, _), &cell) in columns.iter().zip(&cells[1..]) {
        frame.render_widget(right((*name).to_string()).style(dim), cell);
    }
    if has_bar {
        axis(frame.buffer_mut(), cells[cells.len() - 1], dim, scale);
    }

    for (stage, row) in stages.iter().zip(rows) {
        let cells = split(row);
        frame.render_widget(Line::from(stage.name).style(dim), cells[0]);
        for ((_, pick), &cell) in columns.iter().zip(&cells[1..]) {
            let text = match &stage.spread {
                Some(spread) => format!("{:.2}", ms(pick(spread))),
                None => "—".to_string(),
            };
            frame.render_widget(right(text), cell);
        }
        if has_bar {
            range(frame, cells[cells.len() - 1], stage, scale);
        }
    }
}

fn ms(at: Duration) -> f32 {
    at.as_secs_f32() * 1e3
}

/// The latency bars' shared log scale, from `config.lua`.
#[derive(Clone, Copy)]
struct Scale {
    floor_ms: f32,
    ceil_ms: f32,
}

impl Scale {
    fn at(self, ms: f32) -> f32 {
        meter::ms_scale(ms, self.floor_ms, self.ceil_ms)
    }

    fn decades(self) -> Vec<f32> {
        meter::decades(self.floor_ms, self.ceil_ms)
    }
}

/// The cell `scaled` falls in across `width`.
fn cell_at(scaled: f32, width: u16) -> u16 {
    ((scaled * f32::from(width)) as u16).min(width.saturating_sub(1))
}

/// Decade labels across the bar column, each left out where it would run
/// into the one before or off the end.
fn axis(buffer: &mut ratatui::buffer::Buffer, area: Rect, style: Style, scale: Scale) {
    let mut free = area.left();
    let decades = scale.decades();
    for (n, &decade) in decades.iter().enumerate() {
        let mut label = meter::decade_label(decade);
        if n + 1 == decades.len() {
            label.push_str("ms");
        }
        let width = label.chars().count() as u16;
        let x = (area.left() + cell_at(scale.at(decade), area.width))
            .min(area.right().saturating_sub(width));
        if x >= free && x + width <= area.right() {
            buffer.set_string(x, area.top(), &label, style);
            free = x + width + 1;
        }
    }
}

/// p50 as the fill, coloured by the stage's budget; then decade ticks on the
/// empty track, a heavy p99 tick, and a light max marker, written over it
/// the way `shadow` writes, with max winning where they share a cell.
fn range(frame: &mut Frame, area: Rect, stage: &Stage, scale: Scale) {
    let (warn, danger) = (
        scale.at(stage.budget.warn_ms),
        scale.at(stage.budget.danger_ms),
    );
    let p50 = stage.spread.map_or(0.0, |s| scale.at(ms(s.p50)));
    frame.render_widget(bar(String::new(), p50, warn, danger), area);

    let buffer = frame.buffer_mut();
    let y = area.top();
    let filled = p50 * f32::from(area.width);
    let inner = scale
        .decades()
        .into_iter()
        .filter(|&decade| decade > scale.floor_ms && decade < scale.ceil_ms);
    for decade in inner {
        let at = cell_at(scale.at(decade), area.width);
        if f32::from(at) < filled {
            continue;
        }
        if let Some(cell) = buffer.cell_mut((area.left() + at, y)) {
            cell.set_symbol("·");
        }
    }

    let Some(spread) = stage.spread else {
        return;
    };
    let p99 = scale.at(ms(spread.p99));
    let marks = [("┃", health(p99, warn, danger)), ("│", Color::Gray)];
    for ((symbol, colour), at) in marks.into_iter().zip([p99, scale.at(ms(spread.max))]) {
        let x = area.left() + cell_at(at, area.width);
        if let Some(cell) = buffer.cell_mut((x, y)) {
            cell.set_symbol(symbol).set_fg(colour);
        }
    }
}

fn name(note: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!("{}{}", NAMES[(note % 12) as usize], note as i32 / 12 - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_key_survives_the_terminal_and_back() {
        let keys = [
            Key::Char('a'),
            Key::Backspace,
            Key::Delete,
            Key::Enter,
            Key::Tab,
            Key::Esc,
            Key::Up,
            Key::Down,
            Key::Left,
            Key::Right,
            Key::Home,
            Key::End,
            Key::PageUp,
            Key::PageDown,
        ];
        for k in keys {
            assert_eq!(key(code(k)), Some(k));
        }
    }

    #[test]
    fn an_unset_buffer_reads_as_the_default() {
        assert_eq!(buffer(None, 256), ("buffer 256 (default)".into(), false));
        assert_eq!(buffer(None, 0), ("buffer — (default)".into(), false));
    }

    #[test]
    fn a_buffer_given_as_asked_shows_once() {
        assert_eq!(buffer(Some(256), 256), ("buffer 256".into(), false));
        assert_eq!(buffer(Some(256), 0), ("buffer 256".into(), false));
    }

    #[test]
    fn a_buffer_other_than_asked_names_both_and_warns() {
        assert_eq!(
            buffer(Some(128), 512),
            ("buffer 512 (asked 128)".into(), true)
        );
    }

    #[test]
    fn a_slow_view_warns_then_turns_red() {
        let config = crate::script::config_defaults().tui;
        let style = |fps| fps_style(fps, &config);
        assert_eq!(style(60), Style::new());
        assert_eq!(style(30), Style::new());
        assert_eq!(style(29), Style::new().fg(Color::Yellow));
        assert_eq!(style(24), Style::new().fg(Color::Yellow));
        assert_eq!(style(23), Style::new().fg(Color::Red));
    }

    #[test]
    fn a_key_no_binding_names_is_dropped() {
        assert_eq!(stroke(KeyEvent::from(KeyCode::F(1))), None);
    }
}
