use anyhow::Context;
use clap::{Parser, Subcommand};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossterm::{
    cursor,
    event::{
        self, Event, KeyCode, KeyModifiers, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
        PushKeyboardEnhancementFlags,
    },
    execute,
    style::Stylize,
    terminal,
};
use std::io;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

const LUA_OSC: &str = include_str!("../lua/osc.lua");
const LUA_VOICE: &str = include_str!("../lua/voice.lua");

// points per waveform cycle: enough that the lowest note still reads
// most of them, small enough that 8 KB stays in L1
const TABLE_SIZE: usize = 2048;

/// synth | rust+luaJIT terminal synthesizer
#[derive(Parser)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Check { path: PathBuf },
    Play { path: PathBuf },
    Doctor,
    Tui,
}

#[derive(Clone, Copy)]
enum Gating {
    Held,
    Fixed(Duration),
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> anyhow::Result<()> {
    let args = Args::parse();
    let gating = detect_gating();
    match args.command {
        Commands::Check { path } => check(&path)?,
        Commands::Play { path } => play(&path)?,
        Commands::Doctor => doctor(gating)?,
        Commands::Tui => tui(gating)?,
    }
    Ok(())
}

// require checks package.preload before any filesystem search, so the
// embedded libraries resolve with no cwd dependency
fn lua_register(lua: &mlua::Lua) -> anyhow::Result<()> {
    let package: mlua::Table = lua.globals().get("package")?;
    let preload: mlua::Table = package.get("preload")?;

    for (name, src) in [("synth.osc", LUA_OSC), ("synth.voice", LUA_VOICE)] {
        let loader = lua.create_function(move |lua, _: mlua::Value| {
            lua.load(src).set_name(name).eval::<mlua::Value>()
        })?;
        preload.set(name, loader)?;
    }
    Ok(())
}

fn check(path: &std::path::Path) -> anyhow::Result<()> {
    let lua = mlua::Lua::new();
    lua_register(&lua)?;

    let patch: mlua::Table = lua.load(path).eval()?;
    let envelope: mlua::Table = patch.get("envelope")?;
    let wave: mlua::Function = patch.get("waveform")?;

    println!("{}", path.display().to_string().bold());
    for key in ["attackMS", "decayMS", "sustainLevel", "releaseMS"] {
        let value: f64 = envelope.get(key)?;
        println!("  {key:<14}{value}");
    }
    for phase in [0.0, 0.25, 0.5, 0.75] {
        let value: f64 = wave.call(phase)?;
        println!("  wave({phase:.2})      {value}");
    }
    Ok(())
}

// the instrument: timbre and envelope shape, shared by every note
struct Voice {
    wave: Vec<f32>,
    attack: Vec<f32>,
    decay: Vec<f32>,
    sustain_level: f32,
    release: Vec<f32>,
}

fn bake(lua: &mlua::Lua, path: &std::path::Path, sample_rate: u32) -> anyhow::Result<Voice> {
    let patch: mlua::Table = lua.load(path).eval()?;
    let wave_fn: mlua::Function = patch.get("waveform")?;
    let env: mlua::Table = patch.get("envelope")?;

    let mut wave = Vec::with_capacity(TABLE_SIZE);
    for i in 0..TABLE_SIZE {
        let value: f64 = wave_fn.call(i as f64 / TABLE_SIZE as f64)?;
        wave.push((value as f32).clamp(-1.0, 1.0));
    }

    let sustain_level: f64 = env.get("sustainLevel")?;

    // Rust owns the sample rate, so it sizes each segment and lets the
    // patch's own curve function fill it
    let curve = |ms_key: &str, fn_key: &str, from: f64, to: f64| -> anyhow::Result<Vec<f32>> {
        let ms: f64 = env.get(ms_key)?;
        let shape: mlua::Function = env.get(fn_key)?;
        let steps = (sample_rate as f64 * ms / 1000.0) as usize;
        let filled: mlua::Table = shape.call((from, to, steps))?;

        let mut out = Vec::with_capacity(steps);
        for i in 1..=steps {
            let value: f64 = filled.get(i)?;
            out.push(value as f32);
        }
        Ok(out)
    };

    Ok(Voice {
        wave,
        attack: curve("attackMS", "attackFunc", 0.0, 1.0)?,
        decay: curve("decayMS", "decayFunc", 1.0, sustain_level)?,
        sustain_level: sustain_level as f32,
        release: curve("releaseMS", "releaseFunc", sustain_level, 0.0)?,
    })
}

enum Stage {
    Attack,
    Decay,
    Sustain,
    Release,
    Done,
}

// one sounding note
struct Note {
    voice: Voice,
    phase: f32,
    step: f32,
    stage: Stage,
    idx: usize,
    held: usize,
    hold_samples: usize,
    level: f32,
    release_scale: f32,
}

impl Note {
    fn new(voice: Voice, freq: f32, sample_rate: f32, hold: Duration) -> Self {
        Self {
            step: freq / sample_rate,
            hold_samples: (hold.as_secs_f32() * sample_rate) as usize,
            voice,
            phase: 0.0,
            stage: Stage::Attack,
            idx: 0,
            held: 0,
            level: 0.0,
            release_scale: 1.0,
        }
    }

    // release starts from wherever the envelope actually was, so a key
    // lifted mid-attack does not jump up to sustain level first
    fn off(&mut self) {
        self.release_scale = if self.voice.sustain_level > 0.0 {
            self.level / self.voice.sustain_level
        } else {
            1.0
        };
        self.stage = Stage::Release;
        self.idx = 0;
    }

    // the loop falls through zero-length segments, e.g. attackMS = 0
    fn envelope(&mut self) -> f32 {
        loop {
            match self.stage {
                Stage::Attack => {
                    if let Some(&value) = self.voice.attack.get(self.idx) {
                        self.idx += 1;
                        self.level = value;
                        return value;
                    }
                    self.stage = Stage::Decay;
                    self.idx = 0;
                }
                Stage::Decay => {
                    if let Some(&value) = self.voice.decay.get(self.idx) {
                        self.idx += 1;
                        self.level = value;
                        return value;
                    }
                    self.stage = Stage::Sustain;
                    self.idx = 0;
                }
                Stage::Sustain => {
                    if self.held >= self.hold_samples {
                        self.off();
                        continue;
                    }
                    self.held += 1;
                    self.level = self.voice.sustain_level;
                    return self.level;
                }
                Stage::Release => {
                    if let Some(&value) = self.voice.release.get(self.idx) {
                        self.idx += 1;
                        return value * self.release_scale;
                    }
                    self.stage = Stage::Done;
                }
                Stage::Done => return 0.0,
            }
        }
    }

    // reads wave.len() rather than TABLE_SIZE so differently sized tables
    // play without changing this
    fn wave(&self) -> f32 {
        let n = self.voice.wave.len();
        let pos = self.phase * n as f32;
        let i = pos as usize % n;
        let j = (i + 1) % n;
        self.voice.wave[i] + (self.voice.wave[j] - self.voice.wave[i]) * (pos - pos.floor())
    }

    fn next(&mut self) -> f32 {
        let env = self.envelope();
        let sample = self.wave() * env;
        self.phase = (self.phase + self.step).fract();
        sample
    }
}

fn play(path: &std::path::Path) -> anyhow::Result<()> {
    let device = cpal::default_host()
        .default_output_device()
        .context("no default output device")?;
    let supported = device.default_output_config()?;
    anyhow::ensure!(
        supported.sample_format() == cpal::SampleFormat::F32,
        "device wants {:?}, only f32 is wired up",
        supported.sample_format()
    );

    let sample_rate = supported.sample_rate();
    let channels = supported.channels() as usize;

    let lua = mlua::Lua::new();
    lua_register(&lua)?;
    let voice = bake(&lua, path, sample_rate)?;

    let hold = Duration::from_millis(1200);
    let mut note = Note::new(voice, 440.0, sample_rate as f32, hold);

    let stream = device.build_output_stream(
        supported.into(),
        move |out: &mut [f32], _: &cpal::OutputCallbackInfo| {
            for frame in out.chunks_mut(channels) {
                let sample = note.next();
                for channel in frame.iter_mut() {
                    *channel = sample;
                }
            }
        },
        |err| eprintln!("stream error: {err}"),
        None,
    )?;
    stream.play()?;

    println!("440 Hz · {sample_rate} Hz · {channels} ch");
    std::thread::sleep(hold + Duration::from_millis(800));
    Ok(())
}

fn tui(gating: Gating) -> anyhow::Result<()> {
    // setup
    terminal::enable_raw_mode()?;
    execute!(io::stdout(), terminal::EnterAlternateScreen)?;
    enable_gating(gating)?;

    // run + result
    let result = tui_loop();

    // teardown
    let _ = disable_gating(gating);
    let _ = execute!(io::stdout(), terminal::LeaveAlternateScreen);
    let _ = terminal::disable_raw_mode();
    result
}

fn tui_loop() -> anyhow::Result<()> {
    let mut last = String::from("(none)");

    loop {
        execute!(
            io::stdout(),
            terminal::Clear(terminal::ClearType::All),
            cursor::MoveTo(0, 0)
        )?;
        // NOTE: raw mode stops translating \n, so \r\n is needed to return to column 0
        print!("last key: {last}\r\n");
        print!("esc or ctrl-c/d to quit\r\n");

        if event::poll(Duration::from_millis(16))? {
            if let Event::Key(key) = event::read()? {
                // NOTE: contains(), not ==, so ctrl+shift+c still counts as ctrl
                let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
                match key.code {
                    KeyCode::Esc => break,
                    KeyCode::Char('c' | 'd') if ctrl => break,
                    // NOTE: key.kind is Press/Repeat/Release — Release only with gating held
                    _ => last = format!("{:?} / {:?}", key.code, key.kind),
                }
            }
        }
    }
    Ok(())
}

fn enable_gating(gating: Gating) -> io::Result<()> {
    if let Gating::Held = gating {
        execute!(
            io::stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::REPORT_EVENT_TYPES)
        )?;
    }
    Ok(())
}

fn disable_gating(gating: Gating) -> io::Result<()> {
    if let Gating::Held = gating {
        execute!(io::stdout(), PopKeyboardEnhancementFlags)?;
    }
    Ok(())
}

fn doctor(gating: Gating) -> io::Result<()> {
    let term = std::env::var("TERM").unwrap_or_default();
    let in_tmux = std::env::var("TMUX").is_ok();

    println!("{}", "Doctor".bold().blue());
    println!("{}{}", "     terminal:  ".bold(), term);
    println!(
        "{}{}",
        "  multiplexer:  ".bold(),
        if in_tmux { "tmux" } else { "none" }
    );

    match gating {
        Gating::Held => {
            println!("{}{}", "  key release:  ".bold(), "yes".green());
            println!("{}{}", "    gating:       ".bold(), "held notes");
        }
        Gating::Fixed(length) => {
            println!("{}{}", "  key release:  ".bold(), "no".red());
            println!(
                "{}{} {} ms",
                "       gating:  ".bold(),
                "fixed".yellow(),
                length.as_millis()
            );
        }
    }

    Ok(())
}

fn detect_gating() -> Gating {
    match crossterm::terminal::supports_keyboard_enhancement() {
        Ok(true) => Gating::Held,
        _ => Gating::Fixed(Duration::from_millis(400)),
    }
}
