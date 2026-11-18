// The colour triangle's picture: the pure hue at the right corner, white at the top left and black
// at the bottom left, with a pixel's soft edge round it. Without the ring the triangle is centred
// across the square; with it (`v.ring`), it sits in the middle of a hue ring round it, red at the
// right and the hues going anticlockwise. The app draws it on the GPU for each hue, given the
// plugin's controls as `v`; where touches land is the plugin's code (src/lib.rs), which has the
// same sizes.

// Its height across the square: an equilateral triangle of side 1.
const TRIANGLE: f32 = 0.8660254;
// With the ring: the ring from this far from the middle out to the edge, the corners this far.
const RING_INNER: f32 = 0.4;
const RING_TRIANGLE: f32 = 0.37;
const TURN: f32 = 6.2831853;

// The corners: the pure hue, white and black.
fn corner(which: i32) -> vec2<f32> {
    if (v.ring > 0.5) {
        let r = RING_TRIANGLE;
        let rise = RING_TRIANGLE * TRIANGLE;
        if (which == 0) {
            return vec2<f32>(0.5 + r, 0.5);
        }
        if (which == 1) {
            return vec2<f32>(0.5 - r / 2.0, 0.5 - rise);
        }
        return vec2<f32>(0.5 - r / 2.0, 0.5 + rise);
    }
    let left = (1.0 - TRIANGLE) / 2.0;
    if (which == 0) {
        return vec2<f32>(left + TRIANGLE, 0.5);
    }
    if (which == 1) {
        return vec2<f32>(left, 0.0);
    }
    return vec2<f32>(left, 1.0);
}

// How much of the pure hue, white and black make point `p`: they add up to 1, and one below 0 is
// outside.
fn shares(p: vec2<f32>) -> vec3<f32> {
    let pure = corner(0);
    let white = corner(1);
    let black = corner(2);
    let det = (white.y - black.y) * (pure.x - black.x) + (black.x - white.x) * (pure.y - black.y);
    let first = ((white.y - black.y) * (p.x - black.x) + (black.x - white.x) * (p.y - black.y)) / det;
    let second = ((black.y - pure.y) * (p.x - black.x) + (pure.x - black.x) * (p.y - black.y)) / det;
    return vec3<f32>(first, second, 1.0 - first - second);
}

// Hue `h`, saturation `s` and brightness `b` (each 0..1) as red, green and blue.
fn hsv_rgb(h: f32, s: f32, b: f32) -> vec3<f32> {
    let n = (vec3<f32>(5.0, 3.0, 1.0) + vec3<f32>(fract(h) * 6.0)) % vec3<f32>(6.0);
    let k = clamp(min(n, vec3<f32>(4.0) - n), vec3<f32>(0.0), vec3<f32>(1.0));
    return vec3<f32>(b) - b * s * k;
}

// The triangle at `p`, its edge soft over a pixel.
fn triangle_at(p: vec2<f32>) -> vec4<f32> {
    let w = shares(p);
    // how far in from the nearest side, in pixels: a share times the triangle's height
    var tall = TRIANGLE;
    if (v.ring > 0.5) {
        tall = RING_TRIANGLE * 1.5;
    }
    let inside = min(w.x, min(w.y, w.z)) * tall * picture_size().x;
    let edge = clamp(inside + 0.5, 0.0, 1.0);
    let c = max(w, vec3<f32>(0.0));
    let bright = clamp(c.x + c.y, 0.0, 1.0);
    var s = 0.0;
    if (bright > 0.000001) {
        s = clamp(c.x / bright, 0.0, 1.0);
    }
    return vec4<f32>(hsv_rgb(hue(), s, bright), edge);
}

fn picture(p: vec2<f32>) -> vec4<f32> {
    let tri = triangle_at(p);
    if (v.ring <= 0.5) {
        return tri;
    }
    // the ring, soft over a pixel at both its edges, round from red at the right anticlockwise
    let d = p - vec2<f32>(0.5);
    let far = length(d);
    let px = picture_size().x;
    let band = clamp((0.5 - far) * px + 0.5, 0.0, 1.0) * clamp((far - RING_INNER) * px + 0.5, 0.0, 1.0);
    if (band <= tri.a) {
        return tri;
    }
    return vec4<f32>(hsv_rgb(fract(atan2(-d.y, d.x) / TURN), 1.0, 1.0), band);
}
