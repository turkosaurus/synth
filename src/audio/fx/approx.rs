//! Cheap stand-ins for `std` float functions, which `core` lacks.
//! Each is pinned against `std` in `tests.rs`, so their error is measured.

use core::f32::consts::{FRAC_PI_2, FRAC_PI_4};

/// `tan(x)` for `x` in `0.0..=0.49 * PI`, which covers every cutoff from
/// 0 Hz to just under Nyquist.
///
/// A Padé approximant is accurate near 0 and drifts as `x` grows. So above
/// `PI / 4` the identity `tan(x) = 1 / tan(PI / 2 - x)` folds `x` back into
/// the accurate half. Error stays under 0.1%.
pub(crate) fn tan(x: f32) -> f32 {
    if x <= FRAC_PI_4 {
        pade(x)
    } else {
        1.0 / pade(FRAC_PI_2 - x)
    }
}

// (15x - x³) / (15 - 6x²), the [3/2] Padé approximant of tan
fn pade(x: f32) -> f32 {
    let x2 = x * x;
    x * (15.0 - x2) / (15.0 - 6.0 * x2)
}
