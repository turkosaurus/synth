use std::sync::Arc;
use std::time::Duration;

use crossterm::{cursor, event, execute, terminal};

use crate::audio::{self, Event, Player, Stats};

use crate::input::keyboard::{Gating, Keyboard, Outcome};
use crate::script;

const QUEUE: usize = 256;
const POLL: Duration = Duration::from_millis(16);

pub fn run(path: Option<&std::path::Path>, gating: Gating) -> anyhow::Result<()> {
    let output = audio::open()?;
    let patch = match path {
        Some(path) => script::load(path, output.sample_rate)?,
        None => script::load_default(output.sample_rate)?,
    };

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

    crate::terminal::enter(gating)?;
    let result = pump(&mut keyboard, &stats, gating);
    crate::terminal::leave(gating);
    result
}

fn pump(
    keyboard: &mut Keyboard,
    stats: &Stats,
    gating: Gating,
) -> anyhow::Result<()> {
    loop {
        draw(keyboard, stats, gating)?;

        if event::poll(POLL)? {
            // drain everything queued, so held chords and fast playing do not
            // back up one event per frame
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
    keyboard: &Keyboard,
    stats: &Stats,
    gating: Gating,
) -> anyhow::Result<()> {
    let sounding = match keyboard.sounding() {
        Some(note) => format!(
            "{} ({:.1} Hz)",
            name(note),
            440.0 * 2f32.powf((note as f32 - 69.0) / 12.0)
        ),
        None => "—".to_string(),
    };
    let mode = match gating {
        Gating::Held => "held".to_string(),
        Gating::Fixed(length) => format!("fixed {}ms", length.as_millis()),
    };

    execute!(
        std::io::stdout(),
        terminal::Clear(terminal::ClearType::All),
        cursor::MoveTo(0, 0)
    )?;

    // raw mode stops translating \n, so \r\n is needed to return to column 0
    print!("  note    {sounding}\r\n");
    print!(
        "  key     {}      octave  {:+}\r\n",
        keyboard.last_key().unwrap_or(' '),
        keyboard.octave()
    );
    print!("  gating  {mode}\r\n\r\n");
    print!("  {}\r\n\r\n", super::report::stats(&stats.snapshot()));
    print!("  esc panics · ctrl-c/d quits · -/= octave\r\n");
    Ok(())
}

fn name(note: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!("{}{}", NAMES[(note % 12) as usize], note as i32 / 12 - 1)
}
