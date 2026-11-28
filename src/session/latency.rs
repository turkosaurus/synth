//! In-process stage timings as the session keeps them for a view: each
//! stage's readings from the last span of time, and their spread.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::audio::Record;

/// One stage's readings from the last `span`, each with when it landed.
/// Bounded by time rather than count, so every stage covers the same span
/// and one that stops reporting empties.
pub(crate) struct Window<T> {
    span: Duration,
    values: VecDeque<(Instant, T)>,
}

/// Where a window's readings fall, by nearest rank.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Spread<T> {
    pub(crate) p50: T,
    pub(crate) p99: T,
    pub(crate) max: T,
}

impl<T: Copy + Ord> Window<T> {
    pub(crate) fn new(span: Duration) -> Self {
        Self {
            span,
            values: VecDeque::new(),
        }
    }

    pub(crate) fn push(&mut self, at: Instant, value: T) {
        self.values.push_back((at, value));
        self.prune(at);
    }

    /// Drops the readings older than the span before `now`.
    pub(crate) fn prune(&mut self, now: Instant) {
        while self
            .values
            .front()
            .is_some_and(|&(at, _)| now.saturating_duration_since(at) > self.span)
        {
            self.values.pop_front();
        }
    }

    /// `None` when the window holds no readings.
    pub(crate) fn spread(&self) -> Option<Spread<T>> {
        let mut sorted: Vec<T> = self.values.iter().map(|&(_, value)| value).collect();
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
    /// Every stage keeps the readings from the last `span`.
    pub(crate) fn new(span: Duration) -> Self {
        Self {
            input: Window::new(span),
            render: Window::new(span),
            output: Window::new(span),
            total: Window::new(span),
            view: Rate::default(),
        }
    }

    /// Folds in `record`, as read at `at`.
    pub(crate) fn record(&mut self, at: Instant, record: Record) {
        match record {
            Record::Key {
                input,
                wait,
                output,
            } => {
                self.input.push(at, input);
                self.render.push(at, wait);
                self.total.push(at, input + wait + output);
            }
            Record::Output(latency) => self.output.push(at, latency),
        }
    }

    /// Drops every stage's readings older than its span before `now`.
    pub(crate) fn prune(&mut self, now: Instant) {
        for window in [
            &mut self.input,
            &mut self.render,
            &mut self.output,
            &mut self.total,
        ] {
            window.prune(now);
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
mod tests;
