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

/// Hands out each scripted result in turn, then fails.
struct Script(Vec<anyhow::Result<Vec<KeyEvent>>>);

impl Source for Script {
    fn keys(&mut self, _: Duration, out: &mut Vec<(KeyEvent, Instant)>) -> anyhow::Result<()> {
        if self.0.is_empty() {
            anyhow::bail!("script ran out");
        }
        let keys = self.0.remove(0)?;
        out.extend(keys.into_iter().map(|key| (key, Instant::now())));
        Ok(())
    }
}

fn reads(script: Script) -> anyhow::Result<()> {
    let mut input = Input::detached(crate::session::tests::model());
    let (tx, _rx) = rtrb::RingBuffer::new(GLANCES);
    let mut script = script;
    read(
        &mut input,
        &mut script,
        Duration::ZERO,
        &AtomicBool::new(false),
        tx,
        &std::thread::current(),
    )
}

#[test]
fn the_latest_fault_shows_beside_its_count() {
    let config = crate::script::config_defaults().tui;
    let snap = Snapshot {
        errors: 1,
        ..Snapshot::default()
    };
    let underrun = Fault::Stream(crate::audio::StreamKind::Underrun);
    let line = status(&snap, Some(underrun), None, 60, &config).to_string();
    assert!(
        line.starts_with("errors 1 (last: underrun)  overruns 0  "),
        "{line}"
    );
    let line = status(&snap, None, None, 60, &config).to_string();
    assert!(line.starts_with("errors 1  overruns 0  "), "{line}");
}

#[test]
fn tee_drops_and_input_errors_have_counts_that_warn() {
    let config = crate::script::config_defaults().tui;
    let snap = Snapshot {
        output_tee_dropped: 3,
        ..Snapshot::default()
    };
    let tee = Fault::TeeDropped { frames: 3 };
    let line = status(&snap, Some(tee), None, 60, &config);
    let span = |name: &str| {
        line.spans
            .iter()
            .find(|span| span.content.starts_with(name))
            .unwrap()
            .clone()
    };
    assert_eq!(
        span("tee dropped").content,
        "tee dropped 3 (last: output tee dropped 3 frames)  "
    );
    assert_eq!(span("tee dropped").style, Style::new().fg(Color::Yellow));
    assert_eq!(span("input errors").content, "input errors 0  ");
    assert_eq!(span("input errors").style, Style::new());
}

#[test]
fn an_input_error_keeps_its_kind() {
    let error = anyhow::Error::from(std::io::Error::from(std::io::ErrorKind::UnexpectedEof))
        .context("reading key");
    assert_eq!(io_kind(&error), std::io::ErrorKind::UnexpectedEof);
    assert_eq!(io_kind(&anyhow::anyhow!("eof")), std::io::ErrorKind::Other);
}

#[test]
fn an_input_error_ends_the_read_loop() {
    let script = Script(vec![Ok(vec![]), Err(anyhow::anyhow!("eof"))]);
    let err = reads(script).unwrap_err();
    assert_eq!(err.to_string(), "eof");
}

#[test]
fn ctrl_c_ends_the_read_loop() {
    let quit = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert!(reads(Script(vec![Ok(vec![quit])])).is_ok());
}

#[test]
fn a_key_no_binding_names_is_dropped() {
    assert_eq!(stroke(KeyEvent::from(KeyCode::F(1))), None);
}

fn keymap() -> Keymap {
    crate::session::tests::model().keyboard.keymap().clone()
}

/// `draw` run on a `width` by `height` terminal, as text.
fn drawn(width: u16, height: u16, draw: impl FnOnce(&mut Frame)) -> String {
    let backend = ratatui::backend::TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(draw).unwrap();
    let buffer = terminal.backend().buffer();
    buffer
        .content
        .chunks(width as usize)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn help_lists_the_controls_and_no_notes() {
    let text = drawn(80, 24, |frame| {
        popup(frame, " keymap ", help(&keymap()), "");
    });
    assert!(text.contains("octave up     ="), "{text}");
    assert!(text.contains("help          ?"), "{text}");
    assert!(text.contains("quit          q"), "{text}");
    assert!(text.contains("quit now      ctrl-c or ctrl-d"), "{text}");
    assert!(text.contains("octave down   —"), "{text}");
    assert!(!text.contains("C4"), "{text}");
}

#[test]
fn help_reads_quit_off_every_shipped_layout() {
    for layout in ["qwerty", "qwertz", "azerty", "dvorak", "colemak"] {
        let patch = crate::script::tests::with_keyboard(&format!(
            "synth.input.keyboard({{ layout = synth.input.keyboard.{layout} }})"
        ))
        .unwrap();
        let lines: Vec<String> = help(&patch.keymap).iter().map(Line::to_string).collect();
        assert!(
            lines.contains(&"quit          ctrl-q".to_string()),
            "{layout}: {lines:?}"
        );
        assert!(
            lines.contains(&"quit now      ctrl-c or ctrl-d".to_string()),
            "{layout}: {lines:?}"
        );
    }
}

#[test]
fn the_quit_prompt_draws() {
    let text = drawn(40, 10, |frame| {
        popup(frame, " quit ", vec![Line::from("quit? (y/n)")], "");
    });
    assert!(text.contains("quit? (y/n)"), "{text}");
}
