use core::f32::consts::PI;

use super as approx;

#[test]
fn tan_tracks_std() {
    let top = 0.49 * PI;
    for i in 0..=1000 {
        let x = top * i as f32 / 1000.0;
        let want = x.tan();
        let got = approx::tan(x);
        assert!(
            (got - want).abs() <= 1e-3 * want.max(1e-3),
            "tan({x}) = {got}, want {want}"
        );
    }
}

#[test]
fn exp2_tracks_std() {
    for i in -2000..=2000 {
        let x = i as f32 / 100.0;
        let want = x.exp2();
        let got = approx::exp2(x);
        assert!(
            (got - want).abs() <= 1e-4 * want,
            "exp2({x}) = {got}, want {want}"
        );
    }
}

#[test]
fn log2_tracks_std() {
    for i in 1..=4000 {
        let x = i as f32 / 100.0;
        for x in [x, x * 1e-4, x * 1e4] {
            let want = x.log2();
            let got = approx::log2(x);
            assert!((got - want).abs() <= 1e-5, "log2({x}) = {got}, want {want}");
        }
    }
}

#[test]
fn ceil_tracks_std() {
    for i in -1000..=1000 {
        let x = i as f32 / 7.0;
        assert_eq!(approx::ceil(x), x.ceil(), "ceil({x})");
    }
}
