//! Frame sources: the streams a view can read samples from, and which part
//! of one it reads. A view reads `Sources`, never a tee, so another stream
//! joins as one more entry without the view changing.

use std::collections::VecDeque;
use std::fmt;

use crate::audio::OutputTee;

/// Which samples of a stream's frames a view reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pick {
    /// the mean of every channel, so a full-scale frame stays at full scale
    Mix,
    /// one channel, from 0
    Channel(usize),
}

/// A stream, by its index in `Sources`, and the samples read from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Selection {
    pub(crate) stream: usize,
    pub(crate) pick: Pick,
}

impl Selection {
    /// The one after this, given each stream's channel count: a stream's
    /// mix, then each of its channels, then the next stream's mix. A mono
    /// stream has only its mix, which is its one channel.
    pub(crate) fn next(self, channels: &[usize]) -> Self {
        let count = channels.get(self.stream).copied().unwrap_or(0);
        let pick = match self.pick {
            Pick::Mix if count > 1 => Some(Pick::Channel(0)),
            Pick::Channel(i) if i + 1 < count => Some(Pick::Channel(i + 1)),
            _ => None,
        };
        match pick {
            Some(pick) => Self { pick, ..self },
            None => Self {
                stream: (self.stream + 1) % channels.len().max(1),
                pick: Pick::Mix,
            },
        }
    }
}

/// One stream of frames.
pub(crate) struct Stream {
    pub(crate) name: &'static str,
    pub(crate) sample_rate: f32,
    tee: OutputTee,
}

impl Stream {
    pub(crate) fn new(name: &'static str, sample_rate: f32, tee: OutputTee) -> Self {
        Self {
            name,
            sample_rate,
            tee,
        }
    }

    pub(crate) fn channels(&self) -> usize {
        self.tee.channels
    }

    /// Drain every whole frame, handing `out` the picked sample of each
    /// when `pick` is given.
    fn drain(&mut self, pick: Option<Pick>, out: &mut impl FnMut(f32)) {
        let channels = self.tee.channels.max(1);
        let frames = self.tee.samples.slots() / channels;
        let Ok(chunk) = self.tee.samples.read_chunk(frames * channels) else {
            return;
        };
        if let Some(pick) = pick {
            let (head, tail) = chunk.as_slices();
            let mut samples = head.iter().chain(tail).copied();
            for _ in 0..frames {
                let mut sum = 0.0;
                let mut picked = 0.0;
                for (i, sample) in samples.by_ref().take(channels).enumerate() {
                    sum += sample;
                    if pick == Pick::Channel(i) {
                        picked = sample;
                    }
                }
                out(match pick {
                    Pick::Mix => sum / channels as f32,
                    Pick::Channel(_) => picked,
                });
            }
        }
        chunk.commit_all();
    }
}

/// Every stream a view can read, the one it reads now, and its newest
/// samples, which every view of it reads.
pub(crate) struct Sources {
    streams: Vec<Stream>,
    selection: Selection,
    /// oldest first, at most `keep` long
    recent: VecDeque<f32>,
    keep: usize,
}

impl Sources {
    /// `streams`, reading `selection`, which must name one of them.
    pub(crate) fn new(streams: Vec<Stream>, selection: Selection) -> Self {
        assert!(selection.stream < streams.len(), "no such stream");
        Self {
            streams,
            selection,
            recent: VecDeque::new(),
            keep: 0,
        }
    }

    /// Hold the newest `samples` of the selection from the next `read` on,
    /// allocated here, once.
    pub(crate) fn keep(&mut self, samples: usize) {
        self.keep = samples;
        self.recent = VecDeque::with_capacity(samples);
    }

    /// Read `selection` from the next `read` on, forgetting the old one's
    /// samples so no view draws its tail; one naming no stream is ignored.
    pub(crate) fn select(&mut self, selection: Selection) {
        if selection.stream < self.streams.len() && selection != self.selection {
            self.selection = selection;
            self.recent.clear();
        }
    }

    pub(crate) fn sample_rate(&self) -> f32 {
        self.streams[self.selection.stream].sample_rate
    }

    /// Drain every stream, keeping one sample per frame of the selected
    /// one. All of it, every time, and the others unread: a full tee drops
    /// its newest frames and counts them as a fault.
    pub(crate) fn read(&mut self) {
        let selection = self.selection;
        let (recent, keep) = (&mut self.recent, self.keep);
        let mut out = |sample| {
            if recent.len() == keep {
                recent.pop_front();
            }
            if keep > 0 {
                recent.push_back(sample);
            }
        };
        for (i, stream) in self.streams.iter_mut().enumerate() {
            let pick = (i == selection.stream).then_some(selection.pick);
            stream.drain(pick, &mut out);
        }
    }

    /// The newest samples read from the selection, oldest first.
    pub(crate) fn recent(&mut self) -> &[f32] {
        self.recent.make_contiguous()
    }
}

impl fmt::Display for Sources {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = self.streams[self.selection.stream].name;
        match self.selection.pick {
            Pick::Mix => write!(f, "{name} mix"),
            Pick::Channel(i) => write!(f, "{name} ch {}", i + 1),
        }
    }
}

#[cfg(test)]
mod tests;
