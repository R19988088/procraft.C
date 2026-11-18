//! QuickShape curves: a stroke that bends more than once, held, snaps to a curve through its bends
//! rather than a polyline (or, bending one way then the other, rather than an arc), its points
//! there to reshape it. The app hands over the stroke's points and what QuickShape found first
//! (the plugin kit passes them); this smooths the hand's wobble between the stroke's sharp turns,
//! keeps those turns, and lays a chain of cubics along it, adding an anchor where the chain strays
//! from the stroke. The app draws the curve and lets it be reshaped. It's a plugin, not part of the app:
//! `tools/pack_plugin.py` makes a `.procraft-plugin` file of it.
//!
//! A stroke that comes back round to where it began, bending in and out on its way (a cloud, a
//! flower), snaps to a closed curve rather than the ellipse QuickShape makes of it: its last anchor
//! is its first again, smooth there as anywhere. A loop that only bends one way is left the ellipse
//! (or circle) it is.
//!
//! The interface is written down at the top of `crates/core/src/plugin_run.rs`.

/// Closed curves need the same fidelity as open strokes: preserve smaller lobes.
const CLOSED_TOLERANCE: f32 = 0.025;
/// A stroke whose ends are this close (a part of its length) comes back round: QuickShape's own
/// measure.
const CLOSED: f32 = 0.18;
/// A closed stroke's bend turns the other way from the rest when it does by more than this (the
/// sine of the angle): less is a hand's wobble.
const TURNS_BACK: f32 = 0.1;

kit::quickshape_curve!(offer);

/// The curve offered for a stroke of `points`, QuickShape having found `found` first: one for a
/// polyline (it bends more than once), for an arc the stroke doesn't follow (it bends one way and
/// then the other, as no arc does), a closed one for a round loop that bends in and out, or when a
/// curve is asked for by name (none found: closed if the stroke comes back round), else none.
fn offer(points: &[P], found: Option<kit::ShapeKind>) -> Option<Vec<P>> {
    let valid: Vec<P> = points.iter().copied().filter(|p| p.0.is_finite() && p.1.is_finite()).collect();
    let pts = limit_samples(&valid);
    match found {
        None if closed(&pts) => fit_closed(&pts).map(|(nodes, _)| nodes),
        None | Some(kit::ShapeKind::Polyline) => Some(fit(&pts)).filter(|nodes| nodes.len() >= 4),
        Some(kit::ShapeKind::Arc) if bends_both_ways(&pts) => Some(fit(&pts)).filter(|nodes| nodes.len() >= 4),
        Some(kit::ShapeKind::Ellipse | kit::ShapeKind::Circle) if closed(&pts) => {
            fit_closed(&pts).filter(|(_, wavy)| *wavy).map(|(nodes, _)| nodes)
        }
        Some(_) => None,
    }
}

type P = (f32, f32);

fn sub(a: P, b: P) -> P {
    (a.0 - b.0, a.1 - b.1)
}

fn add(a: P, b: P) -> P {
    (a.0 + b.0, a.1 + b.1)
}

fn scale(a: P, k: f32) -> P {
    (a.0 * k, a.1 * k)
}

fn length(a: P) -> f32 {
    a.0.hypot(a.1)
}

/// How far `p` is from the segment `a`–`b`.
fn distance_to_segment(p: P, a: P, b: P) -> f32 {
    let ab = sub(b, a);
    let len2 = ab.0 * ab.0 + ab.1 * ab.1;
    if len2 <= 1e-9 {
        return length(sub(p, a));
    }
    let ap = sub(p, a);
    let t = ((ap.0 * ab.0 + ap.1 * ab.1) / len2).clamp(0.0, 1.0);
    length(sub(p, add(a, scale(ab, t))))
}

/// How big the stroke is: its box's diagonal.
fn size(pts: &[P]) -> f32 {
    let (mut lo, mut hi) = ((f32::MAX, f32::MAX), (f32::MIN, f32::MIN));
    for &(x, y) in pts {
        (lo.0, lo.1, hi.0, hi.1) = (lo.0.min(x), lo.1.min(y), hi.0.max(x), hi.1.max(y));
    }
    if pts.is_empty() { 1.0 } else { length(sub(hi, lo)).max(1.0) }
}

/// The corners that keep the path within `tolerance` (Ramer–Douglas–Peucker), ends included.
fn simplify(pts: &[P], tolerance: f32) -> Vec<P> {
    fn keep(pts: &[P], tolerance: f32, out: &mut Vec<P>) {
        let (Some(&a), Some(&b)) = (pts.first(), pts.last()) else { return };
        let far = pts
            .iter()
            .enumerate()
            .map(|(i, &p)| (i, distance_to_segment(p, a, b)))
            .fold((0, 0.0f32), |best, x| if x.1 > best.1 { x } else { best });
        if far.1 > tolerance && far.0 > 0 && far.0 + 1 < pts.len() {
            keep(&pts[..=far.0], tolerance, out);
            out.pop();
            keep(&pts[far.0..], tolerance, out);
        } else {
            out.push(a);
            out.push(b);
        }
    }
    let mut out = Vec::new();
    keep(pts, tolerance, &mut out);
    out.dedup();
    out
}

/// How far the stroke goes, along its points.
fn path_length(pts: &[P]) -> f32 {
    pts.windows(2).map(|w| length(sub(w[1], w[0]))).sum()
}

/// Whether the stroke turns one way and later clearly the other, as an S does and an arc never
/// does. Its bends are taken finer than a polyline's, so a small deliberate bend counts, but only
/// those with a tenth of the stroke's size or more either side, so the hand's wobble doesn't.
fn bends_both_ways(pts: &[P]) -> bool {
    let scale = size(pts);
    let corners = simplify(pts, 0.045 * scale);
    let (mut left, mut right) = (false, false);
    for w in corners.windows(3) {
        let (before, after) = (sub(w[1], w[0]), sub(w[2], w[1]));
        if length(before) < 0.1 * scale || length(after) < 0.1 * scale {
            continue;
        }
        let sine = (before.0 * after.1 - before.1 * after.0) / (length(before) * length(after)).max(1e-6);
        left |= sine > 0.16;
        right |= sine < -0.16;
    }
    left && right
}

/// Whether the stroke comes back round to where it began, as QuickShape tells a closed one.
fn closed(pts: &[P]) -> bool {
    let (Some(&first), Some(&last)) = (pts.first(), pts.last()) else { return false };
    let way = path_length(pts);
    way > 0.0 && length(sub(last, first)) <= CLOSED * way
}

/// The closed curve round the stroke's bends, and whether it bends in and out (some bend turns
/// the other way from the rest, as an ellipse's never do). None with fewer than three bends.
///
/// The stroke stops where it comes nearest its start again, once past halfway: what runs on
/// beyond lies over its start. The bends are found going once round from the point farthest from
/// the start, so the start, where the stroke closes, is no bend unless it is one; they are laid
/// from the one nearest the start, as the stroke went, and the first again at the end closes it.
fn fit_closed(pts: &[P]) -> Option<(Vec<P>, bool)> {
    let start = *pts.first()?;
    // where it closes: the point nearest its start over the second half of its way
    let half = path_length(pts) / 2.0;
    let mut gone = 0.0;
    let mut close_at = pts.len() - 1;
    let mut nearest = f32::MAX;
    for (i, w) in pts.windows(2).enumerate() {
        gone += length(sub(w[1], w[0]));
        let d = length(sub(w[1], start));
        if gone >= half && d < nearest {
            (close_at, nearest) = (i + 1, d);
        }
    }
    let pts = &pts[..=close_at];
    let far = pts.iter().enumerate().fold((0, 0.0f32), |best, (i, &p)| {
        let d = length(sub(p, start));
        if d > best.1 { (i, d) } else { best }
    });
    // once round, from the far point back to it
    let mut ring: Vec<P> = pts[far.0..].to_vec();
    ring.extend_from_slice(&pts[..=far.0]);
    let mut anchors = simplify(&ring, CLOSED_TOLERANCE * size(pts));
    anchors.pop();
    let n = anchors.len();
    if n < 3 {
        return None;
    }
    let first = (0..n).min_by(|&a, &b| length(sub(anchors[a], start)).total_cmp(&length(sub(anchors[b], start))))?;
    anchors.rotate_left(first);
    let at = |i: usize| anchors[i % n];
    // which way each bend turns, of those that clearly do
    let turns: Vec<f32> = (0..n)
        .filter_map(|i| {
            let (u, v) = (sub(at(i), at(i + n - 1)), sub(at(i + 1), at(i)));
            let sine = (u.0 * v.1 - u.1 * v.0) / (length(u) * length(v)).max(1e-6);
            (sine.abs() > TURNS_BACK).then_some(sine)
        })
        .collect();
    let wavy = turns.iter().any(|&t| t > 0.0) && turns.iter().any(|&t| t < 0.0);
    let mut nodes = Vec::with_capacity(3 * n + 1);
    for i in 0..n {
        let (before, here, after, beyond) = (at(i + n - 1), at(i), at(i + 1), at(i + 2));
        nodes.push(here);
        nodes.push(add(here, scale(sub(after, before), 1.0 / 6.0)));
        nodes.push(sub(after, scale(sub(beyond, here), 1.0 / 6.0)));
    }
    nodes.push(at(0));
    Some((nodes, wavy))
}

/// Fit smooth cubic spans that stay close to the *drawn* stroke. Simply connecting a few
/// Ramer–Douglas–Peucker corners with Catmull–Rom handles changes the path between those corners:
/// it can cut across a wave and even invent a bulge. Instead, measure each proposed cubic's
/// distance from the original samples and insert another anchor where the deviation is greatest.
/// The two controls at a shared anchor use the same tangent, so joins stay smooth.
const MAX_OPEN_SPANS: usize = 64;
/// Bound the work per held stroke regardless of the stylus sampling rate.
const MAX_FIT_POINTS: usize = 256;

fn cubic_at(a: P, b: P, c: P, d: P, t: f32) -> P {
    let u = 1.0 - t;
    add(add(scale(a, u * u * u), scale(b, 3.0 * u * u * t)), add(scale(c, 3.0 * u * t * t), scale(d, t * t * t)))
}

/// Estimate directions over a distance along the stroke, not just three samples. Stylus
/// sampling density varies with speed and otherwise makes the fitted handles wobble.
fn stroke_tangents(pts: &[P], reach: f32) -> Vec<P> {
    let mut along = Vec::with_capacity(pts.len());
    along.push(0.0);
    for pair in pts.windows(2) {
        along.push(along.last().copied().unwrap_or(0.0) + length(sub(pair[1], pair[0])));
    }
    let (mut before, mut after) = (0, 0);
    (0..pts.len())
        .map(|i| {
            while before + 1 < i && along[i] - along[before + 1] >= reach {
                before += 1;
            }
            while after + 1 < pts.len() && along[after] - along[i] < reach {
                after += 1;
            }
            let direction = sub(pts[after], pts[before]);
            let magnitude = length(direction);
            if magnitude > 1e-6 { scale(direction, 1.0 / magnitude) } else { (0.0, 0.0) }
        })
        .collect()
}

fn cubic_span(pts: &[P], outgoing: &[P], incoming: &[P], a: usize, b: usize) -> [P; 4] {
    let start = pts[a];
    let end = pts[b];
    let reach = length(sub(end, start)) / 3.0;
    [start, add(start, scale(outgoing[a], reach)), sub(end, scale(incoming[b], reach)), end]
}

/// Distance to the sampled cubic instead of matching parameter t: pointer samples have varying
/// speed, and an unequal speed must not count as a geometric error.
fn cubic_deviation(pts: &[P], a: usize, b: usize, nodes: [P; 4]) -> (usize, f32) {
    let steps = 10;
    let mut last = nodes[0];
    let mut outline = Vec::with_capacity(steps);
    for i in 1..=steps {
        let now = cubic_at(nodes[0], nodes[1], nodes[2], nodes[3], i as f32 / steps as f32);
        outline.push((last, now));
        last = now;
    }
    let mut worst = (a, 0.0);
    for (i, &p) in pts.iter().enumerate().take(b).skip(a + 1) {
        let distance = outline.iter().map(|&(x, y)| distance_to_segment(p, x, y)).fold(f32::INFINITY, f32::min);
        if distance > worst.1 {
            worst = (i, distance);
        }
    }
    worst
}

/// Bound fitting time on dense stylus strokes by resampling at equal distances along the
/// path. This preserves the stroke's ends and large bends, while removing redundant samples.
/// Without this bound, adaptive curve fitting can exhaust the plugin's execution budget and
/// ProCraft permanently switches off the curves plugin, falling back to polylines.
fn limit_samples(pts: &[P]) -> Vec<P> {
    if pts.len() <= MAX_FIT_POINTS {
        return pts.to_vec();
    }
    let mut along = Vec::with_capacity(pts.len());
    along.push(0.0_f32);
    for pair in pts.windows(2) {
        along.push(along.last().copied().unwrap_or(0.0) + length(sub(pair[1], pair[0])));
    }
    let length = along.last().copied().unwrap_or(0.0);
    if length <= 1e-6 {
        return vec![pts[0], pts[pts.len() - 1]];
    }
    let mut out = Vec::with_capacity(MAX_FIT_POINTS);
    let mut after = 1;
    for i in 0..MAX_FIT_POINTS {
        let target = length * i as f32 / (MAX_FIT_POINTS - 1) as f32;
        while after + 1 < pts.len() && along[after] < target {
            after += 1;
        }
        let before = after - 1;
        let span = along[after] - along[before];
        let t = if span > 1e-6 { ((target - along[before]) / span).clamp(0.0, 1.0) } else { 0.0 };
        out.push(add(pts[before], scale(sub(pts[after], pts[before]), t)));
    }
    out[0] = pts[0];
    out[MAX_FIT_POINTS - 1] = pts[pts.len() - 1];
    out
}

/// A bounded smoothing radius removes small hand tremors without shifting the broad
/// shape of the stroke. Its units are *distance along the pen path*, not input samples.
/// Hard corners are fitted separately, so this filter never crosses a sharp turn.
fn smoothing_radius(pts: &[P]) -> f32 {
    (size(pts) * 0.021).clamp(5.5, 11.0)
}

fn smooth_stroke(pts: &[P]) -> Vec<P> {
    smooth_stroke_with_sigma(pts, smoothing_radius(pts))
}

fn smooth_stroke_with_sigma(pts: &[P], sigma: f32) -> Vec<P> {
    if pts.len() < 4 {
        return pts.to_vec();
    }
    let radius = sigma * 2.5;
    let mut along = Vec::with_capacity(pts.len());
    along.push(0.0f32);
    for pair in pts.windows(2) {
        along.push(along.last().copied().unwrap_or(0.0) + length(sub(pair[1], pair[0])));
    }
    if along.last().copied().unwrap_or(0.0) < radius * 2.0 {
        return pts.to_vec();
    }
    let mut out = pts.to_vec();
    let (mut left, mut right) = (0, 0);
    for i in 1..pts.len() - 1 {
        while left + 1 < i && along[i] - along[left] > radius {
            left += 1;
        }
        while right + 1 < pts.len() && along[right + 1] - along[i] <= radius {
            right += 1;
        }
        let (mut sum, mut weight_sum) = ((0.0, 0.0), 0.0);
        for j in left..=right {
            let delta = (along[j] - along[i]) / sigma;
            let weight = (-0.5 * delta * delta).exp();
            sum = add(sum, scale(pts[j], weight));
            weight_sum += weight;
        }
        if weight_sum > 0.0 {
            out[i] = scale(sum, 1.0 / weight_sum);
        }
    }
    // Holding a stroke should not move the places where the pen actually began and ended.
    out
}

/// Find *supported* hard turns in the sampled path, without classifying tiny hand
/// tremors as corners. Simplification proposes candidates; the direction over a longer
/// stretch of path on either side must change sharply for a candidate to count.
fn hard_corners(pts: &[P]) -> Vec<usize> {
    if pts.len() < 5 {
        return Vec::new();
    }
    let scale = size(pts);
    // Corner candidates come from a *lightly* smoothed path, not raw noisy samples.
    // This discards brief stylus wiggles but preserves sustained direction changes.
    let guide = smooth_stroke_with_sigma(pts, 3.5);
    let candidates = simplify(&guide, (scale * 0.014).clamp(2.0, 5.0));
    let reach = (scale * 0.047).clamp(13.0, 26.0);
    let mut found = Vec::new();
    for corner in candidates.iter().skip(1).take(candidates.len().saturating_sub(2)) {
        let Some(i) = guide.iter().position(|p| p == corner) else { continue };
        if i == 0 || i + 1 >= pts.len() {
            continue;
        }
        let mut before = i;
        let mut after = i;
        let (mut gone_back, mut gone_forward) = (0.0, 0.0);
        while before > 0 && gone_back < reach {
            gone_back += length(sub(guide[before], guide[before - 1]));
            before -= 1;
        }
        while after + 1 < pts.len() && gone_forward < reach {
            gone_forward += length(sub(guide[after + 1], guide[after]));
            after += 1;
        }
        if gone_back < reach * 0.85 || gone_forward < reach * 0.85 {
            continue;
        }
        let into = sub(guide[i], guide[before]);
        let out = sub(guide[after], guide[i]);
        let dot = (into.0 * out.0 + into.1 * out.1) / (length(into) * length(out)).max(1e-6);
        if dot >= 0.55 {
            continue;
        }
        // A smoothly curved wave can change heading by 60+ degrees across a broad
        // distance, especially at its peaks. A *corner* turns abruptly on a shorter
        // scale as well. Check both scales to retain bends but not false hard corners.
        let near_reach = (scale * 0.014).clamp(5.0, 9.0);
        let (mut left, mut right) = (i, i);
        let (mut left_path, mut right_path) = (0.0, 0.0);
        while left > 0 && left_path < near_reach {
            left_path += length(sub(guide[left], guide[left - 1]));
            left -= 1;
        }
        while right + 1 < pts.len() && right_path < near_reach {
            right_path += length(sub(guide[right + 1], guide[right]));
            right += 1;
        }
        if left_path < near_reach * 0.8 || right_path < near_reach * 0.8 {
            continue;
        }
        let u = sub(guide[i], guide[left]);
        let v = sub(guide[right], guide[i]);
        let near_dot = (u.0 * v.0 + u.1 * v.1) / (length(u) * length(v)).max(1e-6);
        if near_dot < 0.55 && found.last().is_none_or(|&previous| previous != i) {
            found.push(i);
        }
    }
    found
}

/// Smooth each stretch separately. The Gaussian kernel must not cross a hard corner,
/// and the exact corner position must survive smoothing and adaptive Bezier fitting.
fn smooth_between_corners(pts: &[P], corners: &[usize]) -> Vec<P> {
    let mut smoothed = pts.to_vec();
    let mut start = 0;
    for &end in corners.iter().chain(std::iter::once(&(pts.len() - 1))) {
        if start < end {
            smoothed[start..=end].copy_from_slice(&smooth_stroke(&pts[start..=end]));
        }
        start = end;
    }
    smoothed
}

fn fit(pts: &[P]) -> Vec<P> {
    let valid: Vec<P> = pts.iter().copied().filter(|p| p.0.is_finite() && p.1.is_finite()).collect();
    if valid.len() < 2 {
        return Vec::new();
    }
    let sampled = limit_samples(&valid);
    let corners = hard_corners(&sampled);
    let pts = smooth_between_corners(&sampled, &corners);
    let tangent_reach = smoothing_radius(&pts) * 1.5;
    let mut incoming = vec![(0.0, 0.0); pts.len()];
    let mut outgoing = vec![(0.0, 0.0); pts.len()];
    let mut start = 0;
    for &end in corners.iter().chain(std::iter::once(&(pts.len() - 1))) {
        if start < end {
            let local = stroke_tangents(&pts[start..=end], tangent_reach);
            for (at, &direction) in local.iter().enumerate() {
                let i = start + at;
                if i != start || start == 0 {
                    incoming[i] = direction;
                }
                if i != end || end == pts.len() - 1 {
                    outgoing[i] = direction;
                }
            }
        }
        start = end;
    }
    // Fit the broad centreline, rather than approximating every small variation in it.
    // This allows slightly more positional freedom in exchange for a fairer, simpler curve.
    let tolerance = (size(&pts) * 0.012).clamp(3.0, 7.0);
    let mut anchors = Vec::with_capacity(corners.len() + 2);
    anchors.push(0);
    anchors.extend(corners);
    anchors.push(pts.len() - 1);
    // each span's farthest point and how far the cubic is from it: worked out again only for the
    // two spans an added anchor splits one into, so a wavy stroke's many spans cost little
    let stray = |a: usize, b: usize| {
        if b - a < 2 { (a, 0.0) } else { cubic_deviation(&pts, a, b, cubic_span(&pts, &outgoing, &incoming, a, b)) }
    };
    let mut strays: Vec<(usize, f32)> = anchors.windows(2).map(|w| stray(w[0], w[1])).collect();
    while anchors.len() <= MAX_OPEN_SPANS {
        let (mut worst, mut span) = ((0, 0.0), 0);
        for (k, &(at, error)) in strays.iter().enumerate() {
            if error > worst.1 {
                (worst, span) = ((at, error), k);
            }
        }
        if worst.1 <= tolerance || worst.0 == 0 {
            break;
        }
        let (a, b) = (anchors[span], anchors[span + 1]);
        if worst.0 <= a || worst.0 >= b {
            break;
        }
        anchors.insert(span + 1, worst.0);
        strays.splice(span..=span, [stray(a, worst.0), stray(worst.0, b)]);
    }
    let mut nodes = Vec::with_capacity(anchors.len() * 3);
    for pair in anchors.windows(2) {
        let [a, b, c, _] = cubic_span(&pts, &outgoing, &incoming, pair[0], pair[1]);
        nodes.extend([a, b, c]);
    }
    nodes.push(pts[*anchors.last().unwrap_or(&0)]);
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stroke across, back and across again.
    fn zigzag() -> Vec<P> {
        let corners = [(0.0, 0.0), (200.0, 10.0), (10.0, 90.0), (200.0, 160.0)];
        corners
            .windows(2)
            .flat_map(|w| (0..25).map(move |i| add(w[0], scale(sub(w[1], w[0]), i as f32 / 25.0))))
            .collect()
    }

    #[test]
    fn an_s_taken_for_an_arc_gets_a_curve_and_a_wobbly_arc_none() {
        let s: Vec<P> = (0..=120)
            .map(|i| {
                let x = 4.0 * i as f32;
                (x, 120.0 + 75.0 * (x * std::f32::consts::TAU / 480.0).sin())
            })
            .collect();
        assert!(bends_both_ways(&s));
        let nodes = offer(&s, Some(kit::ShapeKind::Arc)).expect("a curve for the S");
        assert!(nodes.len() >= 7 && nodes.len() % 3 == 1, "{nodes:?}");
        // a hand's wobble along a real arc turns both ways too, but by too little to count
        let arc: Vec<P> = (0..=80)
            .map(|i| {
                let t = i as f32 / 80.0 * 2.0;
                let wobble = if i % 2 == 0 { 1.5 } else { -1.5 };
                (250.0 + 180.0 * t.cos(), 250.0 + 180.0 * t.sin() + wobble)
            })
            .collect();
        assert!(!bends_both_ways(&arc));
        assert_eq!(offer(&arc, Some(kit::ShapeKind::Arc)), None);
    }

    #[test]
    fn a_sharp_turn_remains_sharp_after_smoothing() {
        let corners = [(0.0, 0.0), (120.0, 0.0), (120.0, 120.0)];
        let stroke: Vec<P> = corners
            .windows(2)
            .flat_map(|w| (0..=40).map(move |i| add(w[0], scale(sub(w[1], w[0]), i as f32 / 40.0))))
            .collect();
        let nodes = fit(&stroke);
        let (at, point) = nodes
            .iter()
            .enumerate()
            .filter(|(i, _)| i % 3 == 0)
            .map(|(i, &p)| (i, p))
            .min_by(|(_, a), (_, b)| length(sub(*a, corners[1])).total_cmp(&length(sub(*b, corners[1]))))
            .unwrap();
        assert!(length(sub(point, corners[1])) < 2.0, "corner moved: {point:?}");
        assert!(at >= 3 && at + 3 < nodes.len());
        let into = sub(nodes[at], nodes[at - 1]);
        let out = sub(nodes[at + 1], nodes[at]);
        let turn_cos = (into.0 * out.0 + into.1 * out.1) / (length(into) * length(out)).max(1e-6);
        assert!(turn_cos < 0.65, "intentionally sharp corner was rounded: {turn_cos}");
    }

    #[test]
    fn smoothing_keeps_a_broad_intentional_bulge_in_the_path() {
        let ideal = |x: f32| 110.0 + 25.0 * (x * std::f32::consts::TAU / 400.0).sin();
        let stroke: Vec<P> = (0..=200)
            .map(|i| {
                let x = i as f32 * 2.0;
                let local = (1.0 - (x - 200.0).abs() / 47.0).max(0.0) * 19.0;
                let wobble = if i == 0 || i == 200 { 0.0 } else { 3.0 * (x * std::f32::consts::TAU / 27.0).sin() };
                (x, ideal(x) + local + wobble)
            })
            .collect();
        let nodes = fit(&stroke);
        let mut output = Vec::new();
        for k in (0..nodes.len() - 1).step_by(3) {
            for j in 0..=24 {
                output.push(cubic_at(nodes[k], nodes[k + 1], nodes[k + 2], nodes[k + 3], j as f32 / 24.0));
            }
        }
        let around_bulge =
            output.iter().filter(|p| (p.0 - 200.0).abs() < 6.0).map(|p| p.1 - ideal(p.0)).fold(0.0f32, f32::max);
        assert!(around_bulge > 10.0, "intentional bulge got erased: {around_bulge:.2} px");
    }

    #[test]
    fn a_wobbly_arc_like_stroke_is_fitted_to_a_consistent_curve() {
        // A moderately uneven freehand circular arc should become a fair curved line,
        // rather than tracing the small oscillations or inventing a sharp angle.
        let centre = (200.0, 200.0);
        let radius = 145.0;
        let stroke: Vec<P> = (0..=180)
            .map(|i| {
                let angle = 0.15 + i as f32 * 2.5 / 180.0;
                let wobble = if i == 0 || i == 180 {
                    0.0
                } else {
                    3.2 * (i as f32 * std::f32::consts::TAU / 13.0).sin()
                        + 2.0 * (i as f32 * std::f32::consts::TAU / 37.0).sin()
                };
                let r = radius + wobble;
                (centre.0 + r * angle.cos(), centre.1 + r * angle.sin())
            })
            .collect();
        let nodes = fit(&stroke);
        let mut output = Vec::new();
        for i in (0..nodes.len() - 1).step_by(3) {
            for k in 0..=24 {
                output.push(cubic_at(nodes[i], nodes[i + 1], nodes[i + 2], nodes[i + 3], k as f32 / 24.0));
            }
        }
        let mean = output.iter().map(|&p| (length(sub(p, centre)) - radius).abs()).sum::<f32>() / output.len() as f32;
        let worst = output.iter().map(|&p| (length(sub(p, centre)) - radius).abs()).fold(0.0f32, f32::max);
        assert!(mean < 3.0, "fitted arc remained uneven: {mean}");
        assert!(worst < 6.0, "fitted arc strayed from its intended shape: {worst}");
        assert_eq!(hard_corners(&limit_samples(&stroke)), Vec::<usize>::new());
    }

    #[test]
    fn nearby_pen_wobble_does_not_create_a_false_corner() {
        let stroke: Vec<P> = (0..=200)
            .map(|i| {
                let x = i as f32 * 2.0;
                (
                    x,
                    100.0
                        + 50.0 * (x * std::f32::consts::TAU / 400.0).sin()
                        + 2.0 * (x * std::f32::consts::TAU / 13.0).sin(),
                )
            })
            .collect();
        let nodes = fit(&stroke);
        for k in (3..nodes.len() - 1).step_by(3) {
            let into = sub(nodes[k], nodes[k - 1]);
            let out = sub(nodes[k + 1], nodes[k]);
            let cross = (into.0 * out.1 - into.1 * out.0).abs() / (length(into) * length(out)).max(1e-6);
            assert!(cross < 0.05, "false corner at {k}: {nodes:?}");
        }
    }

    #[test]
    fn a_stroke_bending_twice_gets_a_curve_through_its_bends() {
        let nodes = fit(&zigzag());
        // anchors at 0, 3, 6 ...: each span's two controls between them; one at each corner
        assert_eq!(nodes.len() % 3, 1, "{nodes:?}");
        assert!(nodes.len() >= 10, "{nodes:?}");
        assert_eq!(nodes[0], (0.0, 0.0));
        // A deliberate zigzag's hard bends should stay non-collinear, not be rounded.
        let (before, at, after) = (nodes[2], nodes[3], nodes[4]);
        let (u, v) = (sub(at, before), sub(after, at));
        let dot = (u.0 * v.0 + u.1 * v.1) / (length(u) * length(v)).max(1e-6);
        assert!(dot < 0.66, "sharp bend was rounded: {u:?} {v:?}");
    }

    #[test]
    fn a_long_wavy_stroke_keeps_its_smaller_bends() {
        // A long stroke whose box is much wider than each individual wave must not have its
        // intermediate turns flattened by the global simplification threshold.
        let stroke: Vec<P> = (0..=240)
            .map(|i| {
                let x = i as f32 * 2.0;
                (x, 100.0 + 18.0 * (x * std::f32::consts::TAU / 120.0).sin())
            })
            .collect();
        let nodes = fit(&stroke);
        let anchors: Vec<P> = nodes.iter().step_by(3).copied().collect();
        assert!(anchors.len() >= 9, "small waves were lost: {anchors:?}");
        for x in [30.0, 90.0, 150.0, 210.0, 270.0, 330.0, 390.0, 450.0] {
            assert!(anchors.iter().any(|&(ax, _)| (ax - x).abs() < 20.0), "missing the bend near {x}: {anchors:?}");
        }
    }

    #[test]
    fn fitted_curve_preserves_a_wavy_strokes_outline_without_new_bumps() {
        let stroke: Vec<P> = (0..=300)
            .map(|i| {
                let x = i as f32 * 2.0;
                (x, 100.0 + 26.0 * (x * std::f32::consts::TAU / 150.0).sin())
            })
            .collect();
        let nodes = fit(&stroke);
        assert_eq!(nodes.len() % 3, 1);
        // The entire fitted outline stays close to what was drawn, not just its anchors.
        let mut samples = Vec::new();
        for base in (0..nodes.len() - 1).step_by(3) {
            for i in 0..=32 {
                samples.push(cubic_at(nodes[base], nodes[base + 1], nodes[base + 2], nodes[base + 3], i as f32 / 32.0));
            }
        }
        let deviation = stroke
            .iter()
            .map(|&p| samples.windows(2).map(|w| distance_to_segment(p, w[0], w[1])).fold(f32::INFINITY, f32::min))
            .fold(0.0f32, f32::max);
        assert!(deviation < 4.0, "fitted path drifted {deviation} px");
        for &(_, y) in &samples {
            assert!((73.0..=127.0).contains(&y), "invented a bulge at y={y}");
        }
        // Both sides of every interior anchor have the same direction (smooth join).
        for k in (3..nodes.len() - 1).step_by(3) {
            let into = sub(nodes[k], nodes[k - 1]);
            let out = sub(nodes[k + 1], nodes[k]);
            assert!((into.0 * out.1 - into.1 * out.0).abs() <= 1e-3 * length(into) * length(out) + 1e-3);
            assert!(into.0 * out.0 + into.1 * out.1 >= -1e-3);
        }
    }

    #[test]
    fn spatial_smoothing_removes_jitter_but_preserves_curve_position() {
        let clean = |x: f32| 100.0 + 30.0 * (x * std::f32::consts::TAU / 200.0).sin();
        let noisy: Vec<P> = (0..=200)
            .map(|i| {
                let x = i as f32 * 2.0;
                let tremor = if i == 0 || i == 200 {
                    0.0
                } else if i % 2 == 0 {
                    2.8
                } else {
                    -2.8
                };
                (x, clean(x) + tremor)
            })
            .collect();
        let smooth = smooth_stroke(&noisy);
        assert_eq!(smooth.first(), noisy.first());
        assert_eq!(smooth.last(), noisy.last());
        let mean_error = |pts: &[P]| pts.iter().map(|&(x, y)| (y - clean(x)).abs()).sum::<f32>() / pts.len() as f32;
        assert!(
            mean_error(&smooth) < mean_error(&noisy) * 0.40,
            "jitter was not reduced: {} -> {}",
            mean_error(&noisy),
            mean_error(&smooth)
        );
        assert!(smooth.iter().zip(&noisy).all(|(&a, &b)| length(sub(a, b)) <= 4.0));
        let nodes = fit(&noisy);
        let mut fitted = Vec::new();
        for base in (0..nodes.len() - 1).step_by(3) {
            for i in 0..=20 {
                fitted.push(cubic_at(nodes[base], nodes[base + 1], nodes[base + 2], nodes[base + 3], i as f32 / 20.0));
            }
        }
        let deviation = (0..=200)
            .map(|i| {
                let p = (i as f32 * 2.0, clean(i as f32 * 2.0));
                fitted.windows(2).map(|w| distance_to_segment(p, w[0], w[1])).fold(f32::INFINITY, f32::min)
            })
            .fold(0.0f32, f32::max);
        assert!(deviation < 4.0, "fit drifted {deviation} pixels from intended curve");
    }

    #[test]
    fn a_hand_drawn_wave_becomes_a_clean_curve_without_fitting_its_wobbles() {
        // Two intentional waves with short (17 px) and medium (43 px) wobbles superimposed.
        // The output should follow the broad wave, not precisely reproduce those wobbles.
        let ideal = |x: f32| 160.0 + 60.0 * (x * std::f32::consts::TAU / 300.0).sin();
        let noisy: Vec<P> = (0..=300)
            .map(|i| {
                let x = i as f32 * 2.0;
                let jitter = if i == 0 || i == 300 {
                    0.0
                } else {
                    4.0 * (x * std::f32::consts::TAU / 17.0).sin() + 3.0 * (x * std::f32::consts::TAU / 43.0).sin()
                };
                (x, ideal(x) + jitter)
            })
            .collect();
        let nodes = fit(&noisy);
        assert_eq!(nodes.first(), noisy.first());
        assert_eq!(nodes.last(), noisy.last());
        let mut samples = Vec::new();
        for base in (0..nodes.len() - 1).step_by(3) {
            for i in 0..=24 {
                samples.push(cubic_at(nodes[base], nodes[base + 1], nodes[base + 2], nodes[base + 3], i as f32 / 24.0));
            }
        }
        let mean_drift = samples.iter().map(|&(x, y)| (y - ideal(x)).abs()).sum::<f32>() / samples.len() as f32;
        let max_drift = samples.iter().map(|&(x, y)| (y - ideal(x)).abs()).fold(0.0_f32, f32::max);
        eprintln!("mean drift {mean_drift:.2}, max drift {max_drift:.2}, segments {}", (nodes.len() - 1) / 3);
        assert!(mean_drift < 5.0, "too far from the intended curve: {mean_drift:.2}");
        assert!(max_drift < 11.0, "large unintended deviation: {max_drift:.2}");
        assert!((nodes.len() - 1) / 3 <= 20, "overfitted tiny wobbles with {} segments", (nodes.len() - 1) / 3);
    }

    #[test]
    fn smoothing_is_independent_of_pen_sampling_density() {
        let stroke = |step: usize| -> Vec<P> {
            (0..=600 / step)
                .map(|i| {
                    let x = (i * step) as f32;
                    (
                        x,
                        100.0
                            + 50.0 * (x * std::f32::consts::TAU / 300.0).sin()
                            + 4.0 * (x * std::f32::consts::TAU / 23.0).sin(),
                    )
                })
                .collect()
        };
        let coarse = smooth_stroke(&stroke(3));
        let fine = smooth_stroke(&stroke(1));
        let differences = coarse.iter().enumerate().map(|(i, p)| length(sub(*p, fine[i * 3]))).fold(0.0_f32, f32::max);
        assert!(differences < 2.5, "different sampling densities drifted {differences}");
    }

    #[test]
    fn dense_pen_samples_stay_bounded_without_losing_the_large_curve() {
        // Real tablets can send thousands of samples during one long held stroke.
        // None of their extra sample density should multiply the adaptive fitter's work.
        let stroke: Vec<P> = (0..=4000)
            .map(|i| {
                let x = i as f32 * 0.15;
                (
                    x,
                    170.0
                        + 60.0 * (x * std::f32::consts::TAU / 300.0).sin()
                        + 2.0 * (x * std::f32::consts::TAU / 21.0).sin(),
                )
            })
            .collect();
        let limited = limit_samples(&stroke);
        assert_eq!(limited.len(), MAX_FIT_POINTS);
        assert_eq!(limited[0], stroke[0]);
        assert_eq!(limited[MAX_FIT_POINTS - 1], stroke[stroke.len() - 1]);
        let nodes = offer(&stroke, Some(kit::ShapeKind::Polyline)).unwrap();
        assert!(nodes.len() >= 10);
        assert!(nodes.len() <= MAX_OPEN_SPANS * 3 + 1);
        assert_eq!(nodes[0], stroke[0]);
        assert_eq!(nodes[nodes.len() - 1], stroke[stroke.len() - 1]);
    }

    #[test]
    fn a_curve_is_offered_for_a_polyline_or_when_asked_and_not_otherwise() {
        let stroke = zigzag();
        assert!(offer(&stroke, Some(kit::ShapeKind::Polyline)).is_some_and(|n| n.len() >= 10));
        assert!(offer(&stroke, None).is_some_and(|n| n.len() >= 10));
        assert_eq!(offer(&stroke, Some(kit::ShapeKind::Line)), None);
        assert_eq!(offer(&stroke, Some(kit::ShapeKind::Ellipse)), None);
    }

    /// Once round a loop `r` from (300, 300) at each turn, starting on the left, in `n` steps, and
    /// on a further `over` of the way round (past its start, or short of it if less than 0).
    fn loop_of(r: impl Fn(f32) -> f32, n: usize, over: f32) -> Vec<P> {
        let steps = ((1.0 + over) * n as f32) as usize;
        (0..=steps)
            .map(|i| {
                let t = std::f32::consts::PI + i as f32 / n as f32 * std::f32::consts::TAU;
                (300.0 + r(t) * t.cos(), 300.0 + 0.7 * r(t) * t.sin())
            })
            .collect()
    }

    /// A cloud: a loop bulging out and dipping in five times round.
    fn cloud(over: f32) -> Vec<P> {
        loop_of(|t| 150.0 * (1.0 + 0.25 * (5.0 * t).cos()), 200, over)
    }

    /// Whether `nodes` close: the last anchor the first, the controls either side of it on a line.
    fn closes_smoothly(nodes: &[P]) -> bool {
        let n = nodes.len();
        let (before, at, after) = (nodes[n - 2], nodes[0], nodes[1]);
        let (u, v) = (sub(at, before), sub(after, at));
        n % 3 == 1 && nodes[n - 1] == nodes[0] && (u.0 * v.1 - u.1 * v.0).abs() < 1e-2 * length(u) * length(v)
    }

    #[test]
    fn a_loop_that_bends_in_and_out_closes_as_a_curve_not_an_ellipse() {
        for over in [0.0, 0.1, -0.08] {
            let stroke = cloud(over);
            let nodes = offer(&stroke, Some(kit::ShapeKind::Ellipse)).expect("a closed curve");
            assert!(closes_smoothly(&nodes), "{over}: {nodes:?}");
            // a bend at each bulge and dip at least, and it starts where the stroke did
            assert!(nodes.len() / 3 >= 10, "{over}: {} anchors", nodes.len() / 3);
            assert!(length(sub(nodes[0], stroke[0])) < 0.1 * size(&stroke), "{over}: {:?}", nodes[0]);
            assert!(offer(&stroke, Some(kit::ShapeKind::Circle)).is_some());
        }
        // as it was drawn when reported: a zigzag down and up, round to the right and back along the top
        let corners = [
            (20.0, 0.0),
            (0.0, 60.0),
            (40.0, 40.0),
            (30.0, 100.0),
            (60.0, 60.0),
            (90.0, 90.0),
            (120.0, 30.0),
            (60.0, 0.0),
            (28.0, 4.0),
        ];
        let stroke: Vec<P> = corners
            .windows(2)
            .flat_map(|w| (0..20).map(move |i| add(w[0], scale(sub(w[1], w[0]), i as f32 / 20.0))))
            .collect();
        assert!(offer(&stroke, Some(kit::ShapeKind::Ellipse)).is_some_and(|n| closes_smoothly(&n)));
    }

    #[test]
    fn a_loop_that_only_bends_one_way_is_left_the_ellipse_it_is() {
        let wobbly = loop_of(|t| 150.0 + 4.0 * (9.0 * t).sin(), 200, 0.05);
        assert_eq!(offer(&wobbly, Some(kit::ShapeKind::Ellipse)), None);
        assert_eq!(offer(&wobbly, Some(kit::ShapeKind::Circle)), None);
        // asked for by name, it closes as a curve all the same
        assert!(offer(&wobbly, None).is_some_and(|n| closes_smoothly(&n)));
        // and polygons stay QuickShape's
        assert_eq!(offer(&cloud(0.0), Some(kit::ShapeKind::Quadrilateral)), None);
    }
}
