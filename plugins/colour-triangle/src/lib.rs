//! Colour triangle: the colour panel's Classic tab picks saturation and brightness in a triangle,
//! between the pure hue, white and black, instead of a square; with its ring, the hue on a ring
//! round the triangle instead of the hue slider; and, if asked, the floating colour window shows
//! that picker alone. It's a plugin, not part of the app: `tools/pack_plugin.py` makes a
//! `.procraft-plugin` file of it.
//!
//! On the picker hook: its picture is `picture.wgsl`, which the app draws on the GPU for each hue
//! (given these controls too), and this says whether it draws Classic now, where a touch lands (the
//! saturation and brightness at a point, or the hue on the ring) and where the knobs sit. Points are
//! 0..1 across and down the square.

// no std inside the app; the computer's own for its tests
#![cfg_attr(target_arch = "wasm32", no_std)]

/// The controls by their place, as the manifest starts them.
static CONTROLS: kit::Controls<3> = kit::Controls::new([1.0, 0.0, 0.0]);
kit::controls!(CONTROLS);
const TRIANGLE: usize = 0;
const RING: usize = 1;
const PICKER_ONLY: usize = 2;

/// Its height across the square: an equilateral triangle of side 1.
const HEIGHT: f32 = 0.866_025_4;

/// With the ring: the ring from this far from the middle out to the square's edge, the triangle's
/// corners this far from the middle (a little in from the ring), and the hue's knob on the ring's
/// middle. `picture.wgsl` draws them the same.
const RING_INNER: f32 = 0.4;
const RING_TRIANGLE: f32 = 0.37;
const RING_KNOB: f32 = 0.45;

const TAU: f32 = core::f32::consts::TAU;
const PI: f32 = core::f32::consts::PI;

fn ring() -> bool {
    CONTROLS.on(TRIANGLE) && CONTROLS.on(RING)
}

/// The corners: the pure hue at the right, white at the top left, black at the bottom left. Without
/// the ring the triangle is as wide as it can be, centred across the square; with it, centred in it.
fn corners(ring: bool) -> [[f32; 2]; 3] {
    if ring {
        let (r, half) = (RING_TRIANGLE, RING_TRIANGLE * HEIGHT);
        return [[0.5 + r, 0.5], [0.5 - r / 2.0, 0.5 - half], [0.5 - r / 2.0, 0.5 + half]];
    }
    let left = (1.0 - HEIGHT) / 2.0;
    [[left + HEIGHT, 0.5], [left, 0.0], [left, 1.0]]
}

/// How much of the pure hue, white and black make (x, y): they add up to 1, and one below 0 is
/// outside.
fn shares(corners: [[f32; 2]; 3], x: f32, y: f32) -> [f32; 3] {
    let [p, w, k] = corners;
    let det = (w[1] - k[1]) * (p[0] - k[0]) + (k[0] - w[0]) * (p[1] - k[1]);
    let a = ((w[1] - k[1]) * (x - k[0]) + (k[0] - w[0]) * (y - k[1])) / det;
    let b = ((k[1] - p[1]) * (x - k[0]) + (p[0] - k[0]) * (y - k[1])) / det;
    [a, b, 1.0 - a - b]
}

/// The saturation and brightness at (x, y) of the triangle with `corners`; a point outside is taken
/// at the edge.
fn pick_in(corners: [[f32; 2]; 3], x: f32, y: f32) -> (f32, f32) {
    let parts = shares(corners, x, y).map(|part| part.max(0.0));
    let sum: f32 = parts.iter().sum();
    let [a, b, _] = if sum > 0.0 { parts.map(|part| part / sum) } else { [0.0, 0.0, 1.0] };
    let v = (a + b).clamp(0.0, 1.0);
    (if v > 1e-6 { (a / v).clamp(0.0, 1.0) } else { 0.0 }, v)
}

/// Where saturation `s` and brightness `v` sit in the triangle with `corners`.
fn point_in(corners: [[f32; 2]; 3], s: f32, v: f32) -> (f32, f32) {
    let (s, v) = (s.clamp(0.0, 1.0), v.clamp(0.0, 1.0));
    let [p, w, k] = corners;
    let (a, b, c) = (v * s, v * (1.0 - s), 1.0 - v);
    (a * p[0] + b * w[0] + c * k[0], a * p[1] + b * w[1] + c * k[1])
}

fn pick(x: f32, y: f32) -> (f32, f32) {
    pick_in(corners(ring()), x, y)
}

fn point(s: f32, v: f32) -> (f32, f32) {
    point_in(corners(ring()), s, v)
}

kit::picker!(pick, point);

// Classic is the triangle while its switch is on, and the floating colour window the picker alone
// while that one is too
kit::yes!(procraft_picker_on, || CONTROLS.on(TRIANGLE));
kit::yes!(procraft_floating_picker_only, || CONTROLS.on(TRIANGLE) && CONTROLS.on(PICKER_ONLY));

/// The hue a touch starting at (x, y) picks: with the ring, round it from red at the right,
/// anticlockwise, anywhere outside the triangle's circle; -1 inside it, or without the ring.
fn hue_at(ring: bool, x: f32, y: f32) -> f32 {
    let (dx, dy) = (x - 0.5, y - 0.5);
    let edge = (RING_TRIANGLE + RING_INNER) / 2.0;
    if !ring || dx * dx + dy * dy < edge * edge {
        return -1.0;
    }
    let turn = atan2(-dy, dx) / TAU;
    if turn < 0.0 { turn + 1.0 } else { turn }
}

/// Where hue `h` (0..1) sits on the ring's middle; -1 for each without the ring.
fn hue_point(ring: bool, h: f32) -> (f32, f32) {
    if !ring {
        return (-1.0, -1.0);
    }
    let (sin, cos) = sin_cos(if h.is_finite() { h } else { 0.0 });
    (0.5 + cos * RING_KNOB, 0.5 - sin * RING_KNOB)
}

#[unsafe(no_mangle)]
pub extern "C" fn procraft_picker_hue(x: f32, y: f32) -> f32 {
    hue_at(ring(), x, y)
}

#[unsafe(no_mangle)]
pub extern "C" fn procraft_picker_hue_point(h: f32) -> i64 {
    let (x, y) = hue_point(ring(), h);
    (u64::from(x.to_bits()) | (u64::from(y.to_bits()) << 32)) as i64
}

/// The angle of (x, y) from the x axis, -π to π, to about 1e-6 (no maths library here): the
/// arctangent of the smaller over the larger by a series good to 2e-8 on 0..1.
fn atan2(y: f32, x: f32) -> f32 {
    let (ax, ay) = (abs(x), abs(y));
    let (most, least) = if ax > ay { (ax, ay) } else { (ay, ax) };
    if most <= 0.0 {
        return 0.0;
    }
    let a = least / most;
    let s = a * a;
    let series = [
        -0.333_331_45,
        0.199_935_51,
        -0.142_089,
        0.106_562_64,
        -0.075_289_64,
        0.042_909_614,
        -0.016_165_737,
        0.002_866_225_7,
    ];
    let mut r = a * (1.0 + s * series.iter().rev().fold(0.0, |sum, c| sum * s + c));
    if ay > ax {
        r = PI / 2.0 - r;
    }
    if x < 0.0 {
        r = PI - r;
    }
    if y < 0.0 { -r } else { r }
}

/// The sine and cosine of `turn` turns, to about 1e-5.
fn sin_cos(turn: f32) -> (f32, f32) {
    (sin(turn), sin(turn + 0.25))
}

fn sin(turn: f32) -> f32 {
    // to -½..½ of a turn, then -¼..¼, where the series is close
    let mut t = turn - (turn as i32) as f32;
    if t > 0.5 {
        t -= 1.0;
    } else if t < -0.5 {
        t += 1.0;
    }
    if t > 0.25 {
        t = 0.5 - t;
    } else if t < -0.25 {
        t = -0.5 - t;
    }
    let x = t * TAU;
    let x2 = x * x;
    x * (1.0 - x2 / 6.0 * (1.0 - x2 / 20.0 * (1.0 - x2 / 42.0 * (1.0 - x2 / 72.0 * (1.0 - x2 / 110.0)))))
}

fn abs(v: f32) -> f32 {
    if v < 0.0 { -v } else { v }
}

#[cfg(target_arch = "wasm32")]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_colour_comes_back_where_it_sits() {
        for ring in [false, true] {
            for (s, v) in [(1.0, 1.0), (0.0, 1.0), (0.5, 0.5), (0.25, 0.8), (0.9, 0.1)] {
                let (x, y) = point_in(corners(ring), s, v);
                let (bs, bv) = pick_in(corners(ring), x, y);
                assert!((bs - s).abs() < 1e-3 && (bv - v).abs() < 1e-3, "({s}, {v}) came back ({bs}, {bv})");
            }
            // the corners, and past an edge at the edge
            let [p, w, _] = corners(ring);
            assert_eq!(pick_in(corners(ring), p[0], p[1]), (1.0, 1.0));
            assert_eq!(pick_in(corners(ring), w[0], w[1]), (0.0, 1.0));
            assert_eq!(pick_in(corners(ring), 1.0, 0.5), (1.0, 1.0));
        }
    }

    #[test]
    fn with_the_ring_the_triangle_sits_inside_it_and_the_ring_picks_the_hue() {
        // each corner inside the ring's hole, where a touch picks colour, not hue
        for [x, y] in corners(true) {
            let d = ((x - 0.5).powi(2) + (y - 0.5).powi(2)).sqrt();
            assert!((d - RING_TRIANGLE).abs() < 1e-5 && d < RING_INNER, "{d}");
            assert_eq!(hue_at(true, x, y), -1.0);
        }
        // round the ring from red at the right, anticlockwise
        for (x, y, hue) in [(0.95, 0.5, 0.0), (0.5, 0.05, 0.25), (0.05, 0.5, 0.5), (0.5, 0.95, 0.75), (0.9, 0.1, 0.125)]
        {
            let got = hue_at(true, x, y);
            assert!((got - hue).abs() < 1e-4 || (got - hue).abs() > 0.9999, "({x}, {y}): {got}");
        }
        // and the knob where the hue is, on the ring's middle
        for hue in [0.0, 0.1, 0.25, 0.4, 0.5, 0.66, 0.75, 0.99] {
            let (x, y) = hue_point(true, hue);
            let d = ((x - 0.5).powi(2) + (y - 0.5).powi(2)).sqrt();
            assert!((d - RING_KNOB).abs() < 1e-4, "{hue}: {d}");
            let back = hue_at(true, x, y);
            assert!((back - hue).abs() < 1e-4 || (back - hue).abs() > 0.9999, "{hue}: {back}");
        }
        // without it, the app's slider has the hue
        assert_eq!(hue_point(false, 0.3), (-1.0, -1.0));
        assert_eq!(hue_at(false, 0.95, 0.5), -1.0);
    }

    #[test]
    fn the_series_are_close_enough() {
        for i in -200..=200 {
            let turn = i as f32 / 97.0;
            let (s, c) = sin_cos(turn);
            assert!((s - (turn * TAU).sin()).abs() < 2e-5 && (c - (turn * TAU).cos()).abs() < 2e-5, "{turn}");
            let (y, x) = ((turn * 3.0).sin() * 2.0, (turn * 5.0).cos());
            assert!((atan2(y, x) - y.atan2(x)).abs() < 5e-5, "{y}, {x}");
        }
    }

    #[test]
    fn each_switch_answers_only_under_the_triangle() {
        // the triangle starts on, the others off
        assert_eq!((procraft_picker_on(), procraft_floating_picker_only()), (1, 0));
        procraft_control(PICKER_ONLY as i32, 1.0);
        procraft_control(RING as i32, 1.0);
        assert_eq!((procraft_picker_on(), procraft_floating_picker_only()), (1, 1));
        assert!(procraft_picker_hue(0.95, 0.5) >= 0.0);
        procraft_control(TRIANGLE as i32, 0.0);
        assert_eq!((procraft_picker_on(), procraft_floating_picker_only()), (0, 0));
        assert_eq!(procraft_picker_hue(0.95, 0.5), -1.0);
        procraft_control(TRIANGLE as i32, 1.0);
        procraft_control(RING as i32, 0.0);
        assert_eq!(procraft_picker_hue(0.95, 0.5), -1.0);
    }
}
