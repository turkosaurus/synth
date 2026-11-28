//! Errors from a realtime thread, as plain values.
//! Each producer pushes to its own ring, since a ring takes one producer;
//! the formatting and logging UI thread drains them all.

use std::fmt;
use std::io;
use std::time::Instant;

use tracing::warn;

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
    /// the driver reported an error on the capture stream, or could not
    /// open it, which leaves the session without one
    CaptureStream(StreamKind),
    /// captured frames found the capture tee full
    CaptureTeeDropped { frames: u64 },
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
            Self::Stream(kind) => write!(f, "{kind}"),
            Self::Overrun { used_us, budget_us } => {
                write!(f, "overrun {used_us} of {budget_us} µs")
            }
            Self::Dropped => f.write_str("event dropped, queue full"),
            Self::TeeDropped { frames } => write!(f, "output tee dropped {frames} frames"),
            Self::CaptureStream(kind) => write!(f, "capture stream: {kind}"),
            Self::CaptureTeeDropped { frames } => write!(f, "capture tee dropped {frames} frames"),
            Self::Input(kind) => write!(f, "reading input: {kind}"),
        }
    }
}

impl fmt::Display for StreamKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Underrun => "underrun",
            Self::DeviceGone => "device gone",
            Self::DeviceChanged => "device changed",
            Self::DeviceBusy => "device busy",
            Self::Invalidated => "stream invalidated",
            Self::RealtimeDenied => "realtime denied",
            Self::Backend => "backend error",
            Self::Other => "stream error",
        })
    }
}

/// A fault and when it was pushed, so faults from different rings can be
/// put back in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamped {
    pub at: Instant,
    pub fault: Fault,
}

/// One producer's end of a fault ring.
pub struct Reporter(rtrb::Producer<Stamped>);

impl Reporter {
    /// Push `fault`, stamped now. A full ring loses the record, never the
    /// count, which `Stats` keeps apart.
    pub fn push(&mut self, fault: Fault) {
        let _ = self.0.push(Stamped {
            at: Instant::now(),
            fault,
        });
    }

    /// Faults that fit before the ring is full.
    pub fn slots(&self) -> usize {
        self.0.slots()
    }
}

/// The reading end of every fault ring, for the thread that logs them.
pub struct Faults {
    capacity: usize,
    rings: Vec<rtrb::Consumer<Stamped>>,
    /// faults from every ring, sorted before they are logged
    drained: Vec<Stamped>,
    last: Option<Stamped>,
}

impl Faults {
    /// Each ring made by [`Faults::ring`] holds `capacity` faults.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            rings: Vec::new(),
            drained: Vec::new(),
            last: None,
        }
    }

    /// A new ring for one producer, read by [`Faults::drain`].
    pub fn ring(&mut self) -> Reporter {
        let (tx, rx) = rtrb::RingBuffer::new(self.capacity);
        self.rings.push(rx);
        Reporter(tx)
    }

    /// Log every fault waiting in every ring, oldest first. Faults with
    /// equal stamps keep the order they were drained in, ring by ring, and
    /// the last of them drained is the last shown.
    pub fn drain(&mut self) {
        for ring in &mut self.rings {
            while let Ok(stamped) = ring.pop() {
                self.drained.push(stamped);
            }
        }
        // stable, so equal stamps stay in drain order
        self.drained.sort_by_key(|stamped| stamped.at);
        for stamped in self.drained.drain(..) {
            warn!("{}", stamped.fault);
            // a fault stamped before one already shown may be pushed after
            // that one was drained
            if self.last.is_none_or(|last| last.at <= stamped.at) {
                self.last = Some(stamped);
            }
        }
    }

    /// The most recent fault drained.
    pub fn last(&self) -> Option<Fault> {
        self.last.map(|stamped| stamped.fault)
    }
}

#[cfg(test)]
mod tests;
