//! Stage timing for live events. The UI pushes a [`Stamp`] into its own
//! ring just before each event, and the callback pops one stamp per live
//! event the player applied, so the two rings stay paired in order. Kept
//! out of `render` so the player and `Event` carry no wall-clock time.

use std::time::{Duration, Instant};

/// What put a live event in the ring.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Origin {
    Key,
    /// a `Fixed` note-off from the keyboard's tick, which no key caused
    Tick,
}

/// Pushed beside each live event, before it.
#[derive(Clone, Copy, Debug)]
pub struct Stamp {
    pub pushed: Instant,
    pub origin: Origin,
}

/// One reading from the callback, for a reader off the audio thread.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Record {
    /// a live event as the callback applied it
    Applied {
        origin: Origin,
        /// pushed until the callback that applied it began
        wait: Duration,
        /// frames into that callback's buffer where it landed
        offset: u32,
    },
    /// one callback's `playback - callback`, as the driver reports it
    Output(Duration),
}

/// The callback's end of the timing rings.
pub struct Recorder {
    stamps: rtrb::Consumer<Stamp>,
    records: rtrb::Producer<Record>,
}

/// Rings for `stamps` stamps, which should match the event ring, and
/// `records` records. Returns the stamp producer, the recorder for
/// [`super::stream`], and the record consumer.
pub fn timing(
    stamps: usize,
    records: usize,
) -> (rtrb::Producer<Stamp>, Recorder, rtrb::Consumer<Record>) {
    let (stamp_tx, stamp_rx) = rtrb::RingBuffer::new(stamps);
    let (record_tx, record_rx) = rtrb::RingBuffer::new(records);
    let recorder = Recorder {
        stamps: stamp_rx,
        records: record_tx,
    };
    (stamp_tx, recorder, record_rx)
}

impl Recorder {
    /// Pairs `events` stamps with the live events just applied `offset`
    /// frames into a callback that began at `start`. Returns the records
    /// with no room in the ring.
    pub(super) fn applied(&mut self, events: u32, offset: u32, start: Instant) -> u64 {
        let mut dropped = 0;
        for _ in 0..events {
            // popped even when its record cannot be pushed, so the next
            // stamp still pairs with the next event
            let Ok(stamp) = self.stamps.pop() else {
                break;
            };
            let record = Record::Applied {
                origin: stamp.origin,
                wait: start.saturating_duration_since(stamp.pushed),
                offset,
            };
            dropped += u64::from(self.records.push(record).is_err());
        }
        dropped
    }

    /// Returns 1 when the ring had no room for it.
    pub(super) fn output(&mut self, latency: Duration) -> u64 {
        u64::from(self.records.push(Record::Output(latency)).is_err())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(records: &mut rtrb::Consumer<Record>) -> Vec<Record> {
        std::iter::from_fn(|| records.pop().ok()).collect()
    }

    #[test]
    fn each_applied_event_takes_the_next_stamp() {
        let (mut stamps, mut recorder, mut records) = timing(4, 4);
        let start = Instant::now();
        let early = start - Duration::from_millis(3);
        stamps
            .push(Stamp {
                pushed: early,
                origin: Origin::Key,
            })
            .unwrap();
        stamps
            .push(Stamp {
                pushed: start,
                origin: Origin::Tick,
            })
            .unwrap();

        assert_eq!(recorder.applied(2, 5, start), 0);
        assert_eq!(
            drain(&mut records),
            vec![
                Record::Applied {
                    origin: Origin::Key,
                    wait: Duration::from_millis(3),
                    offset: 5,
                },
                Record::Applied {
                    origin: Origin::Tick,
                    wait: Duration::ZERO,
                    offset: 5,
                },
            ]
        );
    }

    #[test]
    fn a_full_record_ring_still_consumes_the_stamp() {
        let (mut stamps, mut recorder, mut records) = timing(4, 1);
        let start = Instant::now();
        for origin in [Origin::Key, Origin::Tick] {
            stamps
                .push(Stamp {
                    pushed: start,
                    origin,
                })
                .unwrap();
        }
        assert_eq!(recorder.applied(2, 0, start), 1);
        drain(&mut records);
        assert_eq!(recorder.applied(1, 0, start), 0);
        assert_eq!(drain(&mut records), vec![]);
    }
}
