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
/// A socket's name is [`PREFIX`], the synth's pid, then [`SUFFIX`].
const PREFIX: &str = "log-";
const SUFFIX: &str = ".sock";

/// The socket of the synth with process id `pid`.
fn socket_name(pid: u32) -> String {
    format!("{PREFIX}{pid}{SUFFIX}")
}

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

/// Holds the socket file; dropping it removes the file.
pub struct Listening {
    path: PathBuf,
}

impl Drop for Listening {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Listens at `dir/log-<pid>.sock`, on a thread of its own, adding each reader that connects to `socket`. Each synth has
/// its own, so a second one is live too. Sockets nobody answers on were
/// left by a synth that did not exit cleanly, and are removed.
#[cfg(unix)]
pub fn listen(dir: &Path, socket: &Socket) -> anyhow::Result<Listening> {
    use anyhow::Context;
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    use std::os::unix::net::{UnixListener, UnixStream};

    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .with_context(|| format!("create {}", dir.display()))?;
    for stale in sockets(dir) {
        if UnixStream::connect(&stale).is_err() {
            let _ = std::fs::remove_file(&stale);
        }
    }
    let path = dir.join(socket_name(std::process::id()));
    // a reused pid's socket, already found dead above, or racing us
    match std::fs::remove_file(&path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => {
            return Err(e).with_context(|| format!("remove {}", path.display()));
        }
        _ => {}
    }
    let listener = UnixListener::bind(&path).with_context(|| format!("bind {}", path.display()))?;
    let listening = Listening { path: path.clone() };
    // the fallback, `$XDG_STATE_HOME/synth`, may be readable by others
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("restrict {}", path.display()))?;

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

/// The `log-<pid>.sock` files in `dir`, newest first.
#[cfg(unix)]
fn sockets(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with(PREFIX) && name.ends_with(SUFFIX)
        })
        .map(|entry| {
            let made = entry.metadata().and_then(|m| m.modified());
            (made.unwrap_or(std::time::UNIX_EPOCH), entry.path())
        })
        .collect();
    found.sort_by(|a, b| b.cmp(a));
    found.into_iter().map(|(_, path)| path).collect()
}

/// The newest synth's socket in `dir` that answers.
#[cfg(unix)]
fn newest(dir: &Path) -> Option<std::os::unix::net::UnixStream> {
    sockets(dir)
        .iter()
        .find_map(|path| std::os::unix::net::UnixStream::connect(path).ok())
}

/// Each line of the log, live from the newest synth's socket in `sockets`
/// while one runs, otherwise from `file` as it grows, until `line` fails.
/// After the socket closes, the file is followed from its end, so no line
/// is passed twice.
pub fn follow(
    sockets: Option<&Path>,
    file: &Path,
    mut line: impl FnMut(&str) -> io::Result<()>,
) -> io::Result<()> {
    let mut from_end = false;
    #[cfg(unix)]
    if let Some(stream) = sockets.and_then(newest) {
        for text in BufReader::new(stream).lines() {
            line(&text?)?;
        }
        from_end = true;
    }
    #[cfg(not(unix))]
    let _ = sockets;
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
    use super::{listen, newest, socket_name, sockets, Socket};
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
        // left by a synth that did not exit cleanly
        let stale = dir.join(socket_name(1));
        drop(std::os::unix::net::UnixListener::bind(&stale).unwrap());

        let socket = Socket::default();
        let listening = listen(&dir, &socket).unwrap();
        assert!(!stale.exists(), "a dead socket is removed");
        let stream = newest(&dir).expect("our socket answers");
        while socket.readers() == 0 {
            std::thread::sleep(Duration::from_millis(5));
        }

        (&socket).write_all(line(1).as_bytes()).unwrap();
        let mut got = String::new();
        BufReader::new(stream).read_line(&mut got).unwrap();
        assert_eq!(got, line(1));

        drop(listening);
        assert!(sockets(&dir).is_empty(), "the socket is removed on exit");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_reader_connects_to_the_newest_live_synth() {
        use std::os::unix::net::UnixListener;
        let dir = temp("newest");
        let old = UnixListener::bind(dir.join(socket_name(100))).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        let new = UnixListener::bind(dir.join(socket_name(200))).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        drop(UnixListener::bind(dir.join(socket_name(300))).unwrap());

        newest(&dir).expect("a live socket");
        new.set_nonblocking(true).unwrap();
        old.set_nonblocking(true).unwrap();
        assert!(new.accept().is_ok(), "the newest that answers");
        assert!(old.accept().is_err(), "not the older one");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
