//! In-process stage timings as the session keeps them for a view: the
//! latest readings of each stage, and their spread.

use std::collections::VecDeque;
use std::time::Duration;

use crate::audio::{Origin, Record};

/// Readings each stage keeps; the oldest falls out as a new one lands.
const WINDOW: usize = 512;

/// The latest [`WINDOW`] readings of one stage.
pub(crate) struct Window<T> {
    values: VecDeque<T>,
}

impl<T> Default for Window<T> {
    fn default() -> Self {
        Self {
            values: VecDeque::with_capacity(WINDOW),
        }
    }
}

/// Where a window's readings fall, by nearest rank.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Spread<T> {
    pub(crate) p50: T,
    pub(crate) p99: T,
    pub(crate) max: T,
}

impl<T: Copy + Ord> Window<T> {
    pub(crate) fn push(&mut self, value: T) {
        if self.values.len() == WINDOW {
            self.values.pop_front();
        }
        self.values.push_back(value);
    }

    /// `None` before the first reading.
    pub(crate) fn spread(&self) -> Option<Spread<T>> {
        let mut sorted: Vec<T> = self.values.iter().copied().collect();
        sorted.sort_unstable();
        let max = *sorted.last()?;
        let rank = |p: usize| sorted[(sorted.len() * p).div_ceil(100) - 1];
        Some(Spread {
            p50: rank(50),
            p99: rank(99),
            max,
        })
    }
}

/// Every stage a live session times.
#[derive(Default)]
pub(crate) struct Latency {
    /// key read until its event is pushed
    pub(crate) read: Window<Duration>,
    /// pushed until the callback applied it, for key events
    pub(crate) key_wait: Window<Duration>,
    /// the same for `Fixed` note-offs from the tick
    pub(crate) tick_wait: Window<Duration>,
    /// frames into the buffer where a live event landed
    pub(crate) offset: Window<u32>,
    /// `playback - callback`, as the driver reports it
    pub(crate) output: Window<Duration>,
    /// one frame's draw
    pub(crate) draw: Window<Duration>,
}

impl Latency {
    pub(crate) fn record(&mut self, record: Record) {
        match record {
            Record::Applied {
                origin,
                wait,
                offset,
            } => {
                match origin {
                    Origin::Key => self.key_wait.push(wait),
                    Origin::Tick => self.tick_wait.push(wait),
                }
                self.offset.push(offset);
            }
            Record::Output(latency) => self.output.push(latency),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_window_has_no_spread() {
        assert_eq!(Window::<u32>::default().spread(), None);
    }

    #[test]
    fn spread_ranks_the_window() {
        let mut window = Window::default();
        for value in (1..=100).rev() {
            window.push(value);
        }
        assert_eq!(
            window.spread(),
            Some(Spread {
                p50: 50,
                p99: 99,
                max: 100,
            })
        );
    }

    #[test]
    fn the_oldest_reading_falls_out() {
        let mut window = Window::default();
        window.push(1000);
        for _ in 0..WINDOW {
            window.push(1);
        }
        assert_eq!(window.spread().unwrap().max, 1);
    }

    #[test]
    fn tick_events_are_kept_apart_from_keys() {
        let mut latency = Latency::default();
        let wait = Duration::from_millis(2);
        latency.record(Record::Applied {
            origin: Origin::Tick,
            wait,
            offset: 0,
        });
        assert_eq!(latency.key_wait.spread(), None);
        assert_eq!(latency.tick_wait.spread().unwrap().max, wait);
    }
}
