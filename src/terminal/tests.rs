use super::{leave, Guard, ENTERED, RAW};
use std::sync::atomic::Ordering;

// one test, because the steps live in a static the others would race on.
// only RAW is set: with no raw mode really entered, crossterm's undo is a
// no-op, and the escape-code steps would write to the test's stdout.
#[test]
fn each_step_is_undone_once() {
    assert!(!leave(), "nothing entered, nothing to undo");

    ENTERED.store(RAW, Ordering::SeqCst);
    assert!(leave(), "the first leave undoes the step");
    assert!(!leave(), "the second finds nothing, as the hook's would");

    ENTERED.store(RAW, Ordering::SeqCst);
    drop(Guard(()));
    assert_eq!(ENTERED.load(Ordering::SeqCst), 0, "the guard left");
    assert!(!leave(), "and a hook after the guard does nothing");
}
