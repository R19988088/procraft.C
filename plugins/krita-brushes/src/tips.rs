//! Krita's tips: an auto tip drawn from a few numbers (a circle or a rectangle, its edge hard, soft
//! by a curve or Gaussian, flattened, spiked, with holes or noise in it), or pictures (a GIMP brush,
//! a pipe of them, a PNG).

use crate::curve::Curve;
use crate::gimp::Mask;
use crate::xml::Element;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Edge {
    Plain,
    Soft,
    Gauss,
}

/// An auto tip as a preset's `MaskGenerator` gives it.
#[derive(Clone, Debug)]
pub struct AutoTip {
    pub diameter: f32,
    pub ratio: f32,
    rect: bool,
    edge: Edge,
    hfade: f32,
    vfade: f32,
    spikes: u32,
    antialias: bool,
    curve: Option<Curve>,
    /// Holes in it (1 none) and noise (0 none), drawn afresh for every dab.
    pub density: f32,
    pub randomness: f32,
}

impl AutoTip {
    /// The tip a preset's `Brush` element of type `auto_brush` gives.
    pub fn read(brush: &Element) -> Option<AutoTip> {
        let g = brush.child("MaskGenerator")?;
        let edge = match g.attribute("id") {
            Some("soft") => Edge::Soft,
            Some("gauss") => Edge::Gauss,
            _ => Edge::Plain,
        };
        let diameter = g.number("diameter").or_else(|| g.number("radius").map(|r| 2.0 * r))?;
        Some(AutoTip {
            diameter: diameter.max(0.1),
            ratio: g.number("ratio").unwrap_or(1.0).clamp(0.0, 1.0),
            rect: g.attribute("type") == Some("rect"),
            edge,
            hfade: g.number("hfade").unwrap_or(0.0),
            vfade: g.number("vfade").unwrap_or(0.0),
            spikes: g.number("spikes").map_or(2, |s| s.round().max(2.0) as u32),
            antialias: g.attribute("antialiasEdges").is_some_and(|a| a == "1" || a == "true"),
            curve: g.attribute("softness_curve").and_then(Curve::parse).or_else(|| Curve::parse("0,1;1,0;")),
            density: brush.number("density").unwrap_or(1.0).clamp(0.0, 1.0),
            randomness: brush.number("randomness").unwrap_or(0.0).clamp(0.0, 1.0),
        })
    }

    /// Its width and height at its own size (the height its diameter when spiked).
    pub fn size(&self) -> (f32, f32) {
        (self.diameter, if self.spikes == 2 { self.diameter * self.ratio } else { self.diameter })
    }

    /// Whether each dab is drawn afresh (holes or noise).
    pub fn speckled(&self) -> bool {
        self.density < 1.0 || self.randomness > 0.0
    }

    /// The tip drawn `width` pixels across (its height as its ratio says), its edge as soft as
    /// `softness` (1 as it is), holes and noise made with `random`.
    pub fn draw(&self, width: u32, softness: f32, random: &mut dyn FnMut() -> f32) -> Mask {
        let scale = width as f32 / self.diameter;
        let (w, h) = (self.diameter * scale, self.diameter * self.ratio * scale);
        let (tw, th) = self.size();
        let (out_w, out_h) = ((tw * scale).ceil().max(1.0) as u32, (th * scale).ceil().max(1.0) as u32);
        let mut pixels = Vec::with_capacity((out_w * out_h) as usize);
        let shape = Shape::new(self, w.max(1e-3), h.max(1e-3), softness);
        // spikes: the point turned back into the first spike's wedge
        let wedge = std::f32::consts::PI / self.spikes as f32;
        let (cs, ss) = ((-2.0 * wedge).cos(), (-2.0 * wedge).sin());
        for y in 0..out_h {
            for x in 0..out_w {
                // a few samples a pixel where it is small, as Krita takes them
                let n = if self.antialias && (w < 10.0 || h < 10.0) { 3 } else { 1 };
                let mut sum = 0.0;
                for sy in 0..n {
                    for sx in 0..n {
                        let px = x as f32 + (sx as f32 + 0.5) / n as f32 - out_w as f32 * 0.5;
                        let py = y as f32 + (sy as f32 + 0.5) / n as f32 - out_h as f32 * 0.5;
                        let (mut xr, mut yr) = (px, py.abs());
                        if self.spikes > 2 {
                            let mut angle = yr.atan2(xr);
                            while angle > wedge {
                                let (sx0, sy0) = (xr, yr);
                                xr = cs * sx0 - ss * sy0;
                                yr = ss * sx0 + cs * sy0;
                                angle -= 2.0 * wedge;
                            }
                        }
                        sum += shape.cover(xr, yr);
                    }
                }
                let mut v = sum / (n * n) as f32;
                if self.randomness > 0.0 {
                    v *= (1.0 - self.randomness) + self.randomness * random();
                }
                if self.density < 1.0 && v > 0.0 && self.density < random() {
                    v = 0.0;
                }
                pixels.push((v.clamp(0.0, 1.0) * 255.0).round() as u8);
            }
        }
        Mask { width: out_w, height: out_h, pixels }
    }
}

/// An auto tip's shape at a size: how much a point (from its middle, folded into one quarter or
/// spike) is covered.
struct Shape<'a> {
    tip: &'a AutoTip,
    w: f32,
    h: f32,
    /// Its fade's reach across and down (as Krita scales its fade by the softness).
    fade_x: f32,
    fade_y: f32,
}

impl<'a> Shape<'a> {
    fn new(tip: &'a AutoTip, w: f32, h: f32, softness: f32) -> Shape<'a> {
        let soft = 1.0 / softness.max(0.01);
        let fade = |f: f32, size: f32| if f == 0.0 { 1.0 } else { 2.0 / (f * size) };
        Shape { tip, w, h, fade_x: fade(tip.hfade, w) * soft, fade_y: fade(tip.vfade, h) * soft }
    }

    fn cover(&self, x: f32, y: f32) -> f32 {
        let t = self.tip;
        match (t.rect, t.edge) {
            (false, Edge::Plain) => {
                let n = (x * 2.0 / self.w).powi(2) + (y * 2.0 / self.h).powi(2);
                if n > 1.0 {
                    return 0.0;
                }
                let (xr, yr) = if t.antialias { (x.abs() + 1.0, y.abs() + 1.0) } else { (x, y) };
                let nf = (xr * self.fade_x).powi(2) + (yr * self.fade_y).powi(2);
                if nf < 1.0 {
                    return 1.0;
                }
                1.0 - n * (nf - 1.0) / (nf - n)
            }
            (false, Edge::Soft) => {
                let d = (x * 2.0 / self.w).powi(2) + (y * 2.0 / self.h).powi(2);
                if d > 1.0 {
                    return 0.0;
                }
                t.curve.as_ref().map_or(1.0 - d, |c| c.at(d))
            }
            (false, Edge::Gauss) => {
                let fade = (1.0 - (t.hfade + t.vfade) / 2.0).clamp(1e-6, 1.0 - 1e-6);
                let center = (2.5 * (6761.0 * fade - 10000.0)) / (std::f32::consts::SQRT_2 * 6761.0 * fade);
                let factor = std::f32::consts::SQRT_2 * 12500.0 / (6761.0 * fade * self.w / 2.0);
                let ycoef = if t.ratio > 0.0 { 1.0 / t.ratio } else { 1.0 };
                let dist = (x * x + (y * ycoef).powi(2)).sqrt();
                if dist > self.w * 0.5 + 1.0 {
                    return 0.0;
                }
                let d = dist * factor;
                ((erf(d + center) - erf(d - center)) / (2.0 * erf(center))).clamp(0.0, 1.0)
            }
            (true, Edge::Plain) => {
                let (nx, ny) = (x.abs() * 2.0 / self.w, y.abs() * 2.0 / self.h);
                if nx > 1.0 || ny > 1.0 {
                    return 0.0;
                }
                let (xr, yr) = if t.antialias { (x.abs() + 1.0, y.abs() + 1.0) } else { (x.abs(), y.abs()) };
                let (fx, fy) = (xr * self.fade_x, yr * self.fade_y);
                let fxn = nx * (fx - 1.0) / (fx - nx);
                let fyn = ny * (fy - 1.0) / (fy - ny);
                let mut v = 0.0;
                if fx > 1.0 {
                    v = fxn;
                }
                if fxn < fyn && fy > 1.0 {
                    v = fyn;
                }
                1.0 - v
            }
            (true, Edge::Soft) => {
                let c = |v: f32| t.curve.as_ref().map_or(1.0 - v, |c| c.at(v));
                let (sx, sy) = ((x.abs() * 2.0 / self.w).min(1.0), (y.abs() * 2.0 / self.h).min(1.0));
                if x.abs() * 2.0 > self.w || y.abs() * 2.0 > self.h {
                    return 0.0;
                }
                c(sx) * (1.0 - c(1.0 - sx)) * c(sy) * (1.0 - c(1.0 - sy))
            }
            (true, Edge::Gauss) => {
                let xfade = (1.0 - t.hfade / 2.0) * self.w * 0.1;
                let yfade = (1.0 - t.vfade / 2.0) * self.h * 0.1;
                let (xf, yf) = (1.0 / (std::f32::consts::SQRT_2 * xfade), 1.0 / (std::f32::consts::SQRT_2 * yfade));
                let (half_w, half_h) = (self.w * 0.5 - 2.5 * xfade, self.h * 0.5 - 2.5 * yfade);
                if x.abs() > self.w * 0.5 + 1.0 || y.abs() > self.h * 0.5 + 1.0 {
                    return 0.0;
                }
                let norm = 4.0 * erf(half_w * xf) * erf(half_h * yf);
                if !norm.is_finite() || norm == 0.0 {
                    return 0.0;
                }
                let across = erf((half_w + x) * xf) + erf((half_w - x) * xf);
                let down = erf((half_h + y) * yf) + erf((half_h - y) * yf);
                (across * down / norm).clamp(0.0, 1.0)
            }
        }
    }
}

/// The error function, near enough (a few parts in ten million).
fn erf(x: f32) -> f32 {
    let x = f64::from(x);
    let t = 1.0 / (1.0 + 0.5 * x.abs());
    let y = 1.0
        - t * (-x * x - 1.265_512_23
            + t * (1.000_023_68
                + t * (0.374_091_96
                    + t * (0.096_784_18
                        + t * (-0.186_288_06
                            + t * (0.278_868_07
                                + t * (-1.135_203_98
                                    + t * (1.488_515_87 + t * (-0.822_152_23 + t * 0.170_872_77)))))))))
            .exp();
    (if x >= 0.0 { y } else { -y }) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tip(attributes: &str) -> AutoTip {
        let xml = format!(r#"<Brush type="auto_brush"><MaskGenerator {attributes}/></Brush>"#);
        AutoTip::read(&crate::xml::parse(&xml).unwrap()).unwrap()
    }

    #[test]
    fn a_hard_circle_is_full_inside_and_clear_outside() {
        let t = tip(r#"type="circle" id="default" diameter="40" ratio="1" hfade="1" vfade="1" spikes="2""#);
        let m = t.draw(40, 1.0, &mut || 0.5);
        assert_eq!((m.width, m.height), (40, 40));
        assert_eq!(m.pixels[20 * 40 + 20], 255);
        assert_eq!(m.pixels[0], 0);
        // flattened by its ratio
        let flat = tip(r#"type="circle" id="default" diameter="40" ratio="0.5" hfade="1" vfade="1""#).draw(
            40,
            1.0,
            &mut || 0.5,
        );
        assert_eq!(flat.height, 20);
    }

    #[test]
    fn a_gaussian_circle_fades_from_its_middle() {
        let t = tip(r#"type="circle" id="gauss" diameter="100" ratio="1" hfade="0.5" vfade="0.5""#);
        let m = t.draw(100, 1.0, &mut || 0.5);
        let at = |x: usize| m.pixels[50 * 100 + x];
        assert!(at(50) > 240 && at(80) < at(60) && at(99) < 20, "{} {} {} {}", at(50), at(60), at(80), at(99));
        assert!((erf(1.0) - 0.842_700_8).abs() < 1e-5);
    }

    #[test]
    fn holes_take_pixels_out_as_its_density_says() {
        let t = tip(r#"type="rect" id="default" diameter="20" ratio="1" hfade="1" vfade="1""#);
        let mut speckled = AutoTip { density: 0.5, ..t };
        let mut n = 0_u32;
        let m = speckled.draw(20, 1.0, &mut || {
            n = n.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            (n >> 8) as f32 / (1 << 24) as f32
        });
        let painted = m.pixels.iter().filter(|&&p| p > 0).count();
        assert!((120..280).contains(&painted), "{painted}");
        speckled.density = 1.0;
    }
}
