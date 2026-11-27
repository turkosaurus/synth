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

    /// Log every fault waiting in every ring, oldest first.
    pub fn drain(&mut self) {
        for ring in &mut self.rings {
            while let Ok(stamped) = ring.pop() {
                self.drained.push(stamped);
            }
        }
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

    // the later ring is drained first, so only the stamps can put the
    // underrun last
    #[test]
    fn drain_orders_every_ring_by_time() {
        let mut faults = Faults::new(4);
        let mut audio = faults.ring();
        let mut input = faults.ring();
        input.push(Fault::Dropped);
        audio.push(Fault::Stream(StreamKind::Underrun));
        faults.drain();
        assert_eq!(faults.last(), Some(Fault::Stream(StreamKind::Underrun)));
        assert!(faults.rings.iter().all(rtrb::Consumer::is_empty));
    }

    #[test]
    fn a_fault_stamped_earlier_does_not_replace_the_last() {
        let mut faults = Faults::new(4);
        let mut audio = faults.ring();
        let mut input = faults.ring();
        let early = Instant::now();
        audio.push(Fault::Dropped);
        faults.drain();
        input
            .0
            .push(Stamped {
                at: early,
                fault: Fault::Stream(StreamKind::Underrun),
            })
            .unwrap();
        faults.drain();
        assert_eq!(faults.last(), Some(Fault::Dropped));
    }

    #[test]
    fn a_fault_is_plain_data() {
        fn copy<T: Copy + Send>() {}
        copy::<Fault>();
        copy::<Stamped>();
    }
}
