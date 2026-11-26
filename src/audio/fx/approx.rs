//! Cheap stand-ins for `std` float functions, which `core` lacks.
//! Each is pinned against `std` in `tests.rs`, so their error is measured.

use core::f32::consts::{FRAC_PI_2, FRAC_PI_4, LN_2};

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

/// `2^x`. Past ±126 it saturates, which is already past every cutoff.
///
/// The whole part goes straight into the float's exponent bits; a Taylor
/// series covers the fraction, 0 to 1, with error under 0.01%.
pub(crate) fn exp2(x: f32) -> f32 {
    let x = x.clamp(-126.0, 126.0);
    let mut whole = x as i32;
    if whole as f32 > x {
        whole -= 1;
    }
    let f = x - whole as f32;
    // ln(2)^k / k!
    let frac = 1.0
        + f * (LN_2 + f * (0.240_226_5 + f * (0.055_504_1 + f * (0.009_618_1 + f * 0.001_333_4))));
    frac * f32::from_bits(((whole + 127) as u32) << 23)
}
