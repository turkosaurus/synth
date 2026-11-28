use super::*;
use std::time::Duration;

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
            Fault::InputStream(StreamKind::DeviceGone),
            "input stream: device gone",
        ),
        (
            Fault::InputTeeDropped { frames: 2 },
            "input tee dropped 2 frames",
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

fn at(base: Instant, ms: u64, fault: Fault) -> Stamped {
    Stamped {
        at: base + Duration::from_millis(ms),
        fault,
    }
}

// the later ring is drained first, so only the stamps can put the
// underrun last
#[test]
fn drain_orders_every_ring_by_time() {
    let base = Instant::now();
    let mut faults = Faults::new(4);
    let mut audio = faults.ring();
    let mut input = faults.ring();
    let underrun = Fault::Stream(StreamKind::Underrun);
    input.0.push(at(base, 1, Fault::Dropped)).unwrap();
    audio.0.push(at(base, 2, underrun)).unwrap();
    faults.drain();
    assert_eq!(faults.last(), Some(underrun));
    assert!(faults.rings.iter().all(rtrb::Consumer::is_empty));
}

#[test]
fn equal_stamps_keep_drain_order() {
    let base = Instant::now();
    let mut faults = Faults::new(4);
    let mut audio = faults.ring();
    let mut input = faults.ring();
    audio.0.push(at(base, 1, Fault::Dropped)).unwrap();
    input
        .0
        .push(at(base, 1, Fault::Stream(StreamKind::Underrun)))
        .unwrap();
    faults.drain();
    assert_eq!(faults.last(), Some(Fault::Stream(StreamKind::Underrun)));
}

#[test]
fn a_fault_stamped_earlier_does_not_replace_the_last() {
    let base = Instant::now();
    let mut faults = Faults::new(4);
    let mut audio = faults.ring();
    let mut input = faults.ring();
    audio.0.push(at(base, 2, Fault::Dropped)).unwrap();
    faults.drain();
    input
        .0
        .push(at(base, 1, Fault::Stream(StreamKind::Underrun)))
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
