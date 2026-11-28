use super::*;
use crate::audio::output_tee;

fn stereo(frames: &[(f32, f32)]) -> Stream {
    let (mut tx, tee) = output_tee(8, 2);
    for &(left, right) in frames {
        tx.push(left).unwrap();
        tx.push(right).unwrap();
    }
    Stream::new("output", 48_000.0, tee)
}

fn read(sources: &mut Sources) -> Vec<f32> {
    sources.keep(8);
    sources.read();
    sources.recent().to_vec()
}

const MIX: Selection = Selection {
    stream: 0,
    pick: Pick::Mix,
};

#[test]
fn a_channel_reads_only_its_samples() {
    let selection = Selection {
        stream: 0,
        pick: Pick::Channel(1),
    };
    let mut sources = Sources::new(vec![stereo(&[(1.0, -1.0), (0.5, 0.25)])], selection);
    assert_eq!(read(&mut sources), [-1.0, 0.25]);
}

#[test]
fn the_mix_is_the_mean_of_every_channel() {
    let mut sources = Sources::new(vec![stereo(&[(1.0, 1.0), (1.0, 0.0)])], MIX);
    assert_eq!(read(&mut sources), [1.0, 0.5]);
}

#[test]
fn every_stream_is_drained_and_only_the_selected_one_read() {
    let mut sources = Sources::new(
        vec![stereo(&[(1.0, 1.0)]), stereo(&[(0.5, 0.5), (0.5, 0.5)])],
        Selection {
            stream: 1,
            pick: Pick::Mix,
        },
    );
    assert_eq!(read(&mut sources), [0.5, 0.5]);
    assert!(sources.streams.iter().all(|s| s.tee.samples.slots() == 0));
}

#[test]
fn next_walks_each_streams_mix_then_channels_and_wraps() {
    let channels = [2, 1];
    let mut at = MIX;
    let mut seen = Vec::new();
    for _ in 0..5 {
        at = at.next(&channels);
        seen.push(at);
    }
    let pick = |stream, pick| Selection { stream, pick };
    assert_eq!(
        seen,
        [
            pick(0, Pick::Channel(0)),
            pick(0, Pick::Channel(1)),
            pick(1, Pick::Mix),
            pick(0, Pick::Mix),
            pick(0, Pick::Channel(0)),
        ]
    );
}

#[test]
fn a_selection_naming_no_stream_is_ignored() {
    let mut sources = Sources::new(vec![stereo(&[])], MIX);
    sources.select(Selection {
        stream: 3,
        pick: Pick::Mix,
    });
    assert_eq!(sources.selection, MIX);
}

#[test]
fn the_name_says_the_stream_and_the_pick() {
    let mut sources = Sources::new(vec![stereo(&[])], MIX);
    assert_eq!(sources.to_string(), "output mix");
    sources.select(MIX.next(&[2]));
    assert_eq!(sources.to_string(), "output ch 1");
}

#[test]
fn recent_keeps_the_newest_samples() {
    let frames = [(1.0, 1.0), (0.5, 0.5), (0.0, 0.0)];
    let mut sources = Sources::new(vec![stereo(&frames)], MIX);
    sources.keep(2);
    sources.read();
    assert_eq!(sources.recent(), [0.5, 0.0]);
}

#[test]
fn a_new_selection_forgets_the_old_samples() {
    let mut sources = Sources::new(vec![stereo(&[(1.0, 1.0)])], MIX);
    sources.keep(4);
    sources.read();
    sources.select(MIX.next(&[2]));
    assert!(sources.recent().is_empty());
}
