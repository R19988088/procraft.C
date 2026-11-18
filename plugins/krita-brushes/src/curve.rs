//! Krita's response curves: points written `x,y;x,y;`, a smooth spline through them (natural ends),
//! each value held to 0..1, read off a table of 256 steps between them as Krita reads one.

#[derive(Clone, Debug, PartialEq)]
pub struct Curve {
    table: Vec<f32>,
}

const STEPS: usize = 256;

impl Curve {
    /// The curve `text` writes; none if it has fewer than two points, or is the straight line from
    /// 0 to 1 (which changes nothing).
    pub fn parse(text: &str) -> Option<Curve> {
        let mut points: Vec<(f32, f32)> = text
            .split(';')
            .filter_map(|p| {
                let mut n = p.split(',').map(|v| v.trim().parse::<f32>());
                let (x, y) = (n.next()?.ok()?, n.next()?.ok()?);
                (x.is_finite() && y.is_finite()).then_some((x.clamp(0.0, 1.0), y))
            })
            .collect();
        points.sort_by(|a, b| a.0.total_cmp(&b.0));
        points.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-6);
        if points.len() < 2 {
            return None;
        }
        let identity =
            points.len() == 2 && points[0].0 == 0.0 && points[0].1 == 0.0 && points[1].0 == 1.0 && points[1].1 == 1.0;
        if identity {
            return None;
        }
        let spline = Spline::through(&points);
        let table = (0..STEPS).map(|i| spline.at(i as f32 / (STEPS - 1) as f32).clamp(0.0, 1.0)).collect();
        Some(Curve { table })
    }

    /// Its value at `x` (0..1), between the table's two nearest steps.
    pub fn at(&self, x: f32) -> f32 {
        let x = if x.is_finite() { x.clamp(0.0, 1.0) } else { 0.0 };
        let f = x * (STEPS - 1) as f32;
        let i = (f.floor() as usize).min(STEPS - 2);
        let t = f - i as f32;
        self.table[i] * (1.0 - t) + self.table[i + 1] * t
    }
}

/// A cubic spline through points (x rising), its second derivative 0 at both ends.
struct Spline {
    xs: Vec<f32>,
    ys: Vec<f32>,
    second: Vec<f32>,
}

impl Spline {
    fn through(points: &[(f32, f32)]) -> Spline {
        let n = points.len();
        let xs: Vec<f32> = points.iter().map(|p| p.0).collect();
        let ys: Vec<f32> = points.iter().map(|p| p.1).collect();
        let mut second = vec![0.0_f32; n];
        if n > 2 {
            // the tridiagonal system for the inner points' second derivatives, solved forward and back
            let mut diag = vec![0.0_f32; n];
            let mut rhs = vec![0.0_f32; n];
            let h = |i: usize| (xs[i + 1] - xs[i]).max(1e-6);
            for i in 1..n - 1 {
                diag[i] = 2.0 * (h(i - 1) + h(i));
                rhs[i] = 6.0 * ((ys[i + 1] - ys[i]) / h(i) - (ys[i] - ys[i - 1]) / h(i - 1));
            }
            for i in 2..n - 1 {
                let m = h(i - 1) / diag[i - 1];
                diag[i] -= m * h(i - 1);
                rhs[i] -= m * rhs[i - 1];
            }
            for i in (1..n - 1).rev() {
                let next = if i + 1 < n - 1 { second[i + 1] } else { 0.0 };
                second[i] = (rhs[i] - h(i) * next) / diag[i];
            }
        }
        Spline { xs, ys, second }
    }

    fn at(&self, x: f32) -> f32 {
        let n = self.xs.len();
        if x <= self.xs[0] {
            return self.ys[0];
        }
        if x >= self.xs[n - 1] {
            return self.ys[n - 1];
        }
        let i = self.xs.windows(2).position(|w| x <= w[1]).unwrap_or(n - 2);
        let h = (self.xs[i + 1] - self.xs[i]).max(1e-6);
        let a = (self.xs[i + 1] - x) / h;
        let b = (x - self.xs[i]) / h;
        a * self.ys[i]
            + b * self.ys[i + 1]
            + ((a * a * a - a) * self.second[i] + (b * b * b - b) * self.second[i + 1]) * h * h / 6.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_straight_curve_is_none_and_a_bent_one_goes_through_its_points() {
        assert!(Curve::parse("0,0;1,1;").is_none());
        let c = Curve::parse("0,0.25;1,1;").unwrap();
        assert!((c.at(0.0) - 0.25).abs() < 1e-4 && (c.at(1.0) - 1.0).abs() < 1e-4);
        assert!((c.at(0.5) - 0.625).abs() < 1e-3);
        let bent = Curve::parse("0,0;0.5,0.8;1,1;").unwrap();
        assert!((bent.at(0.5) - 0.8).abs() < 1e-2);
    }
}
