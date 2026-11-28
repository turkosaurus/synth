use super::*;

const MS: Duration = Duration::from_millis(1);

#[test]
fn an_empty_window_has_no_spread() {
    assert_eq!(Window::<u32>::new(MS).spread(), None);
}

#[test]
fn spread_ranks_the_window() {
    let mut window = Window::new(Duration::from_secs(1));
    let now = Instant::now();
    for value in (1..=100).rev() {
        window.push(now, value);
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
fn a_reading_older_than_the_span_falls_out() {
    let mut window = Window::new(100 * MS);
    let start = Instant::now();
    window.push(start, 1000);
    window.push(start + 50 * MS, 1);
    window.prune(start + 100 * MS);
    assert_eq!(window.spread().unwrap().max, 1000);
    window.prune(start + 101 * MS);
    assert_eq!(window.spread().unwrap().max, 1);
}

#[test]
fn an_idle_window_empties() {
    let mut window = Window::new(100 * MS);
    let start = Instant::now();
    window.push(start, 1);
    window.prune(start + 200 * MS);
    assert_eq!(window.spread(), None);
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
    let mut latency = Latency::new(Duration::from_secs(1));
    latency.record(
        Instant::now(),
        Record::Key {
            input: MS,
            wait: 2 * MS,
            output: 20 * MS,
        },
    );
    assert_eq!(latency.total.spread().unwrap().max, 23 * MS);
}
