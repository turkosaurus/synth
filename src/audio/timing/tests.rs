use super::*;

fn drain(records: &mut rtrb::Consumer<Record>) -> Vec<Record> {
    std::iter::from_fn(|| records.pop().ok()).collect()
}

const MS: Duration = Duration::from_millis(1);

#[test]
fn each_applied_event_takes_the_next_stamp() {
    let (mut stamps, mut recorder, mut records) = timing(4, 4);
    let start = Instant::now();
    let pushed = start - 3 * MS;
    let read = pushed - MS;
    for origin in [Origin::Key { read }, Origin::Release] {
        stamps.push(Stamp { pushed, origin }).unwrap();
    }

    assert_eq!(recorder.applied(2, start, 20 * MS), 0);
    assert_eq!(
        drain(&mut records),
        vec![Record::Key {
            input: MS,
            wait: 3 * MS,
            output: 20 * MS,
        }]
    );
    assert_eq!(stamps.slots(), 4);
}

#[test]
fn a_full_record_ring_still_consumes_the_stamp() {
    let (mut stamps, mut recorder, mut records) = timing(4, 1);
    let start = Instant::now();
    for _ in 0..2 {
        stamps
            .push(Stamp {
                pushed: start,
                origin: Origin::Key { read: start },
            })
            .unwrap();
    }
    assert_eq!(recorder.applied(2, start, MS), 1);
    drain(&mut records);
    assert_eq!(recorder.applied(1, start, MS), 0);
    assert_eq!(drain(&mut records), vec![]);
}
