//! The log's live copy: a socket carrying each event to whoever is
//! connected, and the reader behind `synth log`.
//!
//! The file is the record; the socket is a view of it that never holds up
//! the writer. A reader that falls behind loses events, never the lines
//! it does get.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tracing_subscriber::fmt::MakeWriter;

/// How often [`follow`] looks for more of the file.
const POLL: Duration = Duration::from_millis(250);
/// One connected reader, with the rest of a line it had no room for.
struct Reader {
    stream: Box<dyn Write + Send>,
    pending: Vec<u8>,
}

impl Reader {
    /// Sends what is pending, then `line` if that went out whole; drops
    /// `line` when the reader is behind. An error means the reader is gone.
    fn send(&mut self, line: &[u8]) -> io::Result<()> {
        let sent = some(&mut self.stream, &self.pending)?;
        self.pending.drain(..sent);
        if !self.pending.is_empty() {
            return Ok(());
        }
        let sent = some(&mut self.stream, line)?;
        if sent > 0 {
            self.pending.extend_from_slice(&line[sent..]);
        }
        Ok(())
    }
}

/// As much of `buf` as `stream` takes now; 0 when it would block.
fn some(stream: &mut dyn Write, buf: &[u8]) -> io::Result<usize> {
    if buf.is_empty() {
        return Ok(0);
    }
    match stream.write(buf) {
        Ok(0) => Err(io::ErrorKind::WriteZero.into()),
        Ok(sent) => Ok(sent),
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(0),
        Err(e) if e.kind() == io::ErrorKind::Interrupted => Ok(0),
        Err(e) => Err(e),
    }
}

/// Fans each event out to the connected readers, as a tracing writer.
/// Clones share one set of readers.
#[derive(Clone, Default)]
pub struct Socket {
    readers: Arc<Mutex<Vec<Reader>>>,
}

impl Socket {
    /// Adds a reader. `stream` must not block, or a slow reader stalls
    /// every thread that logs.
    pub fn add(&self, stream: impl Write + Send + 'static) {
        if let Ok(mut readers) = self.readers.lock() {
            readers.push(Reader {
                stream: Box::new(stream),
                pending: Vec::new(),
            });
        }
    }

    /// How many readers are connected.
    pub fn readers(&self) -> usize {
        self.readers.lock().map_or(0, |readers| readers.len())
    }
}

/// Every write succeeds whole, so the fmt layer hands over one event per
/// call; what each reader cannot take now is its loss, not the writer's.
impl Write for &Socket {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if let Ok(mut readers) = self.readers.lock() {
            readers.retain_mut(|reader| reader.send(buf).is_ok());
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Socket {
    type Writer = &'a Socket;

    fn make_writer(&'a self) -> Self::Writer {
        self
    }
}

/// Holds the socket and its pid file; dropping it removes both.
pub struct Listening {
    socket: PathBuf,
    pid: PathBuf,
}

impl Drop for Listening {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.socket);
        let _ = std::fs::remove_file(&self.pid);
    }
}

/// The pid file beside a socket: `log.pid` for `log.sock`.
fn pid_path(socket: &Path) -> PathBuf {
    socket.with_extension("pid")
}

/// Listens at `path` on a thread of its own, adding each reader that
/// connects to `socket`, and writes this process's pid beside it. A socket
/// that answers belongs to the synth that started first, and is left to it;
/// the error names that synth's pid. One nobody answers on was left by a
/// synth that did not exit cleanly, and is replaced.
#[cfg(unix)]
pub fn listen(path: &Path, socket: &Socket) -> anyhow::Result<Listening> {
    use anyhow::Context;
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    use std::os::unix::net::{UnixListener, UnixStream};

    let pid = pid_path(path);
    if UnixStream::connect(path).is_ok() {
        let holder = std::fs::read_to_string(&pid)
            .ok()
            .and_then(|text| text.trim().parse::<u32>().ok());
        match holder {
            Some(holder) => anyhow::bail!("synth {holder} holds {}", path.display()),
            None => anyhow::bail!("another synth holds {}", path.display()),
        }
    }
    if let Some(dir) = path.parent() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .with_context(|| format!("create {}", dir.display()))?;
    }
    for stale in [path, &pid] {
        match std::fs::remove_file(stale) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => {
                return Err(e).with_context(|| format!("remove {}", stale.display()));
            }
            _ => {}
        }
    }
    let listener = UnixListener::bind(path).with_context(|| format!("bind {}", path.display()))?;
    let listening = Listening {
        socket: path.to_path_buf(),
        pid: pid.clone(),
    };
    // the fallback, `$XDG_STATE_HOME/synth`, may be readable by others
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("restrict {}", path.display()))?;
    std::fs::write(&pid, format!("{}\n", std::process::id()))
        .with_context(|| format!("write {}", pid.display()))?;

    let socket = socket.clone();
    std::thread::Builder::new()
        .name("log socket".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                if stream.set_nonblocking(true).is_ok() {
                    socket.add(stream);
                }
            }
        })
        .context("start the log socket thread")?;
    Ok(listening)
}

/// Each line of the log, live from `socket` while synth runs, otherwise
/// from `file` as it grows, until `line` fails. After the socket closes,
/// the file is followed from its end, so no line is passed twice.
pub fn follow(
    socket: Option<&Path>,
    file: &Path,
    mut line: impl FnMut(&str) -> io::Result<()>,
) -> io::Result<()> {
    let mut from_end = false;
    #[cfg(unix)]
    if let Some(stream) = socket.and_then(|path| std::os::unix::net::UnixStream::connect(path).ok())
    {
        for text in BufReader::new(stream).lines() {
            line(&text?)?;
        }
        from_end = true;
    }
    #[cfg(not(unix))]
    let _ = socket;
    tail(file, from_end, &mut line)
}

/// Follows `path` as `tail -F` does: through its absence, and from the
/// start again once a new session truncates it.
fn tail(
    path: &Path,
    from_end: bool,
    line: &mut impl FnMut(&str) -> io::Result<()>,
) -> io::Result<()> {
    let mut at = None::<u64>;
    let mut partial = Vec::new();
    loop {
        match File::open(path) {
            Ok(mut file) => {
                let len = file.metadata()?.len();
                let start = match at {
                    None if from_end => len,
                    Some(at) if at <= len => at,
                    _ => {
                        partial.clear();
                        0
                    }
                };
                file.seek(SeekFrom::Start(start))?;
                let mut read = Vec::new();
                file.read_to_end(&mut read)?;
                at = Some(start + read.len() as u64);
                partial.extend_from_slice(&read);
                while let Some(end) = partial.iter().position(|&b| b == b'\n') {
                    let text: Vec<u8> = partial.drain(..=end).collect();
                    line(String::from_utf8_lossy(&text[..end]).trim_end_matches('\r'))?;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => at = Some(0),
            Err(e) => return Err(e),
        }
        std::thread::sleep(POLL);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::{listen, pid_path, Socket};
    use std::io::{BufRead, BufReader, Read, Write};
    use std::os::unix::net::UnixStream;
    use std::time::{Duration, Instant};

    fn line(n: usize) -> String {
        format!("event {n:06} {}\n", "x".repeat(200))
    }

    /// Whole lines only: a partial write is finished before the next line
    /// starts, and lines that found no room are dropped entirely.
    fn whole(received: &str) {
        for text in received.lines() {
            assert!(
                text.starts_with("event ") && text.len() == line(0).len() - 1,
                "torn line: {text:?}"
            );
        }
    }

    #[test]
    fn a_reader_that_never_reads_does_not_block_the_writer() {
        let (ours, mut theirs) = UnixStream::pair().unwrap();
        ours.set_nonblocking(true).unwrap();
        let socket = Socket::default();
        socket.add(ours);

        let started = Instant::now();
        // far more than a socket buffer holds
        for n in 0..20_000 {
            (&socket).write_all(line(n).as_bytes()).unwrap();
        }
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(socket.readers(), 1, "a slow reader stays connected");

        theirs.set_nonblocking(true).unwrap();
        let mut received = Vec::new();
        let _ = theirs.read_to_end(&mut received);
        let received = String::from_utf8(received).unwrap();
        assert!(received.lines().count() < 20_000, "some were dropped");

        // the next event finishes any torn line, then goes out whole
        (&socket).write_all(line(20_000).as_bytes()).unwrap();
        let mut rest = Vec::new();
        let _ = theirs.read_to_end(&mut rest);
        let all = received + std::str::from_utf8(&rest).unwrap();
        whole(&all);
        assert!(all.ends_with(&line(20_000)));
    }

    #[test]
    fn a_reader_that_hangs_up_is_dropped() {
        let (ours, theirs) = UnixStream::pair().unwrap();
        ours.set_nonblocking(true).unwrap();
        let socket = Socket::default();
        socket.add(ours);
        drop(theirs);
        (&socket).write_all(line(0).as_bytes()).unwrap();
        assert_eq!(socket.readers(), 0);
    }

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("synth-log-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_connected_reader_gets_each_line() {
        let dir = temp("listen");
        let path = dir.join("log.sock");
        // left by a synth that did not exit cleanly
        drop(std::os::unix::net::UnixListener::bind(&path).unwrap());
        std::fs::write(pid_path(&path), "1\n").unwrap();

        let socket = Socket::default();
        let listening = listen(&path, &socket).unwrap();
        let pid = std::fs::read_to_string(pid_path(&path)).unwrap();
        assert_eq!(pid.trim(), std::process::id().to_string());
        let stream = UnixStream::connect(&path).unwrap();
        while socket.readers() == 0 {
            std::thread::sleep(Duration::from_millis(5));
        }

        (&socket).write_all(line(1).as_bytes()).unwrap();
        let mut got = String::new();
        BufReader::new(stream).read_line(&mut got).unwrap();
        assert_eq!(got, line(1));

        drop(listening);
        assert!(!path.exists() && !pid_path(&path).exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_second_synth_is_told_who_holds_the_socket() {
        let dir = temp("held");
        let path = dir.join("log.sock");
        let first = listen(&path, &Socket::default()).unwrap();
        let Err(error) = listen(&path, &Socket::default()) else {
            panic!("a second listener takes a held socket");
        };
        let holder = format!("synth {} holds", std::process::id());
        assert!(error.to_string().contains(&holder), "{error}");
        assert!(path.exists(), "the holder keeps its socket");
        drop(first);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
