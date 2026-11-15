use core::f32::consts::PI;

use super::approx;

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
