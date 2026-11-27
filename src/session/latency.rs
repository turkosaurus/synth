//! In-process stage timings as the session keeps them for a view: the
//! latest readings of each stage, and their spread.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::audio::Record;

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

/// Every stage a live session times, in signal order.
#[derive(Default)]
pub(crate) struct Latency {
    /// key read until its event is pushed
    pub(crate) input: Window<Duration>,
    /// pushed until the callback applied it
    pub(crate) render: Window<Duration>,
    /// `playback - callback`, as the driver reports it
    pub(crate) output: Window<Duration>,
    /// per key: input, render, and the output of the callback that
    /// applied it
    pub(crate) total: Window<Duration>,
    /// draws in the last second, which no key waits on
    pub(crate) view: Rate,
}

impl Latency {
    pub(crate) fn record(&mut self, record: Record) {
        match record {
            Record::Key {
                input,
                wait,
                output,
            } => {
                self.input.push(input);
                self.render.push(wait);
                self.total.push(input + wait + output);
            }
            Record::Output(latency) => self.output.push(latency),
        }
    }
}

/// Events in the last second.
#[derive(Default)]
pub(crate) struct Rate {
    at: VecDeque<Instant>,
}

impl Rate {
    pub(crate) fn push(&mut self, at: Instant) {
        while self
            .at
            .front()
            .is_some_and(|&first| at.duration_since(first) >= Duration::from_secs(1))
        {
            self.at.pop_front();
        }
        self.at.push_back(at);
    }

    pub(crate) fn per_second(&self) -> usize {
        self.at.len()
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
    fn a_rate_counts_only_the_last_second() {
        let mut rate = Rate::default();
        let start = Instant::now();
        for ms in [0, 500, 999, 1000, 1500] {
            rate.push(start + Duration::from_millis(ms));
        }
        assert_eq!(rate.per_second(), 3);
    }

    #[test]
    fn a_key_total_sums_its_stages() {
        let mut latency = Latency::default();
        let ms = Duration::from_millis(1);
        latency.record(Record::Key {
            input: ms,
            wait: 2 * ms,
            output: 20 * ms,
        });
        assert_eq!(latency.total.spread().unwrap().max, 23 * ms);
    }
}
