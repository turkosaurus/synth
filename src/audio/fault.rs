//! Errors from a realtime thread, as plain values. 
//! Each producer pushes to its own ring, since a ring takes one producer; 
//! the formatting and logging UI thread drains them all.

use std::fmt;
use std::io;

use tracing::warn;

/// Faults each ring holds before the reader drains it. A full ring loses
/// the record, never the count, which `Stats` keeps apart.
const CAPACITY: usize = 64;

/// One fault, copied out of the thread it happened on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Fault {
    /// the driver reported an error on the stream
    Stream(StreamKind),
    /// a callback ran past its deadline
    Overrun { used_us: u32, budget_us: u32 },
    /// a live event found its ring full and was not played
    Dropped,
    /// output frames found the output tee full
    TeeDropped { frames: u64 },
    /// reading input failed, which ends the session
    Input(io::ErrorKind),
}

/// What kind of stream error the driver reported.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum StreamKind {
    Underrun,
    DeviceGone,
    DeviceChanged,
    DeviceBusy,
    Invalidated,
    RealtimeDenied,
    Backend,
    Other,
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Stream(kind) => f.write_str(match kind {
                StreamKind::Underrun => "underrun",
                StreamKind::DeviceGone => "device gone",
                StreamKind::DeviceChanged => "device changed",
                StreamKind::DeviceBusy => "device busy",
                StreamKind::Invalidated => "stream invalidated",
                StreamKind::RealtimeDenied => "realtime denied",
                StreamKind::Backend => "backend error",
                StreamKind::Other => "stream error",
            }),
            Self::Overrun { used_us, budget_us } => {
                write!(f, "overrun {used_us} of {budget_us} µs")
            }
            Self::Dropped => f.write_str("event dropped, queue full"),
            Self::TeeDropped { frames } => write!(f, "output tee dropped {frames} frames"),
            Self::Input(kind) => write!(f, "reading input: {kind}"),
        }
    }
}

/// The reading end of every fault ring, for the thread that logs them.
pub struct Faults {
    rings: Vec<rtrb::Consumer<Fault>>,
    last: Option<Fault>,
}

impl Default for Faults {
    fn default() -> Self {
        Self::new()
    }
}

impl Faults {
    pub fn new() -> Self {
        Self {
            rings: Vec::new(),
            last: None,
        }
    }

    /// A new ring for one producer, read by [`Faults::drain`].
    pub fn ring(&mut self) -> rtrb::Producer<Fault> {
        let (tx, rx) = rtrb::RingBuffer::new(CAPACITY);
        self.rings.push(rx);
        tx
    }

    /// Log every fault waiting in every ring. Rings are drained in turn, so
    /// faults from two producers are not in the order they happened.
    pub fn drain(&mut self) {
        for ring in &mut self.rings {
            while let Ok(fault) = ring.pop() {
                warn!("{fault}");
                self.last = Some(fault);
            }
        }
    }

    /// The most recent fault drained.
    pub fn last(&self) -> Option<Fault> {
        self.last
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_fault_reads_as_text() {
        let cases = [
            (Fault::Stream(StreamKind::Underrun), "underrun"),
            (
                Fault::Overrun {
                    used_us: 1200,
                    budget_us: 1000,
                },
                "overrun 1200 of 1000 µs",
            ),
            (Fault::Dropped, "event dropped, queue full"),
            (
                Fault::TeeDropped { frames: 3 },
                "output tee dropped 3 frames",
            ),
            (
                Fault::Input(io::ErrorKind::UnexpectedEof),
                "reading input: unexpected end of file",
            ),
        ];
        for (fault, text) in cases {
            assert_eq!(fault.to_string(), text);
        }
    }

    #[test]
    fn drain_reads_every_ring_and_keeps_the_latest() {
        let mut faults = Faults::new();
        let mut audio = faults.ring();
        let mut input = faults.ring();
        audio.push(Fault::Stream(StreamKind::Underrun)).unwrap();
        input.push(Fault::Dropped).unwrap();
        faults.drain();
        assert_eq!(faults.last(), Some(Fault::Dropped));
        assert!(faults.rings.iter().all(rtrb::Consumer::is_empty));
    }

    #[test]
    fn a_fault_is_plain_data() {
        fn copy<T: Copy + Send>() {}
        copy::<Fault>();
    }
}
