//! GIMP's brush and pattern files, as Krita reads them for its brushes: a brush (`.gbr`, grey or
//! colour), a pipe (`.gih`, several brushes and how a stroke picks among them) and a pattern
//! (`.pat`).

use crate::files::Rgba;

/// A brush tip as a mask: how much each pixel paints, 0 to 255, rows from the top.
#[derive(Clone, Debug, PartialEq)]
pub struct Mask {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

const MOST_SIDE: u32 = 8192;

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(*b.get(at..)?.first_chunk::<4>()?))
}

/// Qt's grey of a colour, as Krita weighs one.
pub fn grey(r: u8, g: u8, b: u8) -> u8 {
    ((u32::from(r) * 11 + u32::from(g) * 16 + u32::from(b) * 5) / 32) as u8
}

/// A colour picture as a mask, as Krita takes one: dark paints, as far as it is opaque.
#[cfg(test)]
pub fn mask_of(image: &Rgba) -> Mask {
    let pixels = image
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| ((255 - u32::from(grey(p[0], p[1], p[2]))) * u32::from(p[3]) / 255) as u8)
        .collect();
    Mask { width: image.width, height: image.height, pixels }
}

/// Whether every pixel of `image` is grey (its alpha aside).
pub fn all_grey(image: &Rgba) -> bool {
    image.pixels.as_chunks::<4>().0.iter().all(|p| p[0] == p[1] && p[1] == p[2])
}

/// A brush in `bytes` as Krita takes its picture (RGBA, straight: a grey brush opaque, its grey the
/// lightness of what it doesn't paint), whether it is a grey brush, and how many bytes it took;
/// `pixels`: its pixels too, else only its size (read from its header: the rest of it needn't be
/// there).
pub fn gbr(bytes: &[u8], pixels: bool) -> Option<(Rgba, bool, usize)> {
    let header = be32(bytes, 0)? as usize;
    let version = be32(bytes, 4)?;
    let (width, height, depth) = (be32(bytes, 8)?, be32(bytes, 12)?, be32(bytes, 16)?);
    if version == 2 && bytes.get(20..24)? != b"GIMP" {
        return None;
    }
    if !(1..=MOST_SIDE).contains(&width) || !(1..=MOST_SIDE).contains(&height) || header < 20 {
        return None;
    }
    let count = width as usize * height as usize;
    let used = header + count * depth as usize;
    if !pixels {
        return matches!(depth, 1 | 4).then_some((Rgba { width, height, pixels: Vec::new() }, depth == 1, used));
    }
    let data = bytes.get(header..used)?;
    let pixels = match depth {
        1 => data.iter().flat_map(|&v| [255 - v, 255 - v, 255 - v, 255]).collect(),
        4 => data.to_vec(),
        _ => return None,
    };
    Some((Rgba { width, height, pixels }, depth == 1, used))
}

/// How a pipe picks its next cell along one of its dimensions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selection {
    Constant,
    Incremental,
    Angular,
    Velocity,
    Random,
    Pressure,
    TiltX,
    TiltY,
}

/// A pipe: its cells (as [`gbr`] takes them), whether they are grey brushes, and for each
/// dimension how many steps it has and how one is picked.
#[derive(Clone, Debug, PartialEq)]
pub struct Pipe {
    pub cells: Vec<Rgba>,
    pub grey: bool,
    pub ranks: Vec<u32>,
    pub selections: Vec<Selection>,
}

fn selection_named(name: &str) -> Selection {
    match name {
        "incremental" => Selection::Incremental,
        "angular" => Selection::Angular,
        "velocity" => Selection::Velocity,
        "random" => Selection::Random,
        "pressure" => Selection::Pressure,
        "xtilt" => Selection::TiltX,
        "ytilt" => Selection::TiltY,
        _ => Selection::Constant,
    }
}

/// A pipe in `bytes`: a line with its name, a line with its cell count and its settings, then the
/// cells, each a brush; `pixels`: the cells' pixels too, else only their sizes (as many as it
/// says, those past what `bytes` holds of it the size of the last read).
pub fn gih(bytes: &[u8], pixels: bool) -> Option<Pipe> {
    let first = bytes.iter().position(|&b| b == b'\n')?;
    let rest = bytes.get(first + 1..)?;
    let second = rest.iter().position(|&b| b == b'\n')?;
    let line = String::from_utf8_lossy(&rest[..second]).into_owned();
    let mut words = line.split_whitespace();
    let count: usize = words.next()?.parse().ok()?;
    let (mut dim, mut ranks, mut selections) = (1_usize, vec![0_u32; 4], vec![Selection::Incremental; 4]);
    for word in words {
        let Some((key, value)) = word.split_once(':') else { continue };
        if key == "dim" {
            dim = value.parse().unwrap_or(1).clamp(1, 4);
        } else if let Some(i) = key.strip_prefix("rank").and_then(|i| i.parse::<usize>().ok()) {
            if let Some(r) = ranks.get_mut(i) {
                *r = value.parse().unwrap_or(0);
            }
        } else if let Some(i) = key.strip_prefix("sel").and_then(|i| i.parse::<usize>().ok())
            && let Some(s) = selections.get_mut(i)
        {
            *s = selection_named(value);
        }
    }
    let mut cells = Vec::with_capacity(count.min(1024));
    let mut grey = true;
    let mut at = first + 1 + second + 1;
    for _ in 0..count.min(1024) {
        let Some((cell, is_grey, used)) = bytes.get(at..).and_then(|b| gbr(b, pixels)) else { break };
        if cells.is_empty() {
            grey = is_grey;
        }
        cells.push(cell);
        at += used;
    }
    if cells.is_empty() {
        return None;
    }
    if !pixels && at >= bytes.len() {
        let last = cells.last().cloned()?;
        cells.resize(count.min(1024), last);
    }
    ranks.truncate(dim);
    selections.truncate(dim);
    // a pipe whose ranks don't say gives each cell a step of its one dimension
    if ranks.iter().all(|&r| r == 0) {
        ranks = vec![cells.len() as u32];
        selections.truncate(1);
    }
    Some(Pipe { cells, grey, ranks, selections })
}

/// A GIMP pattern in `bytes`: grey, grey and alpha, colour, or colour and alpha; `pixels`: its
/// pixels too, else only its size.
pub fn pat(bytes: &[u8], pixels: bool) -> Option<Rgba> {
    let header = be32(bytes, 0)? as usize;
    let (width, height, depth) = (be32(bytes, 8)?, be32(bytes, 12)?, be32(bytes, 16)?);
    if bytes.get(20..24)? != b"GPAT" || !(1..=MOST_SIDE).contains(&width) || !(1..=MOST_SIDE).contains(&height) {
        return None;
    }
    if !pixels {
        return Some(Rgba { width, height, pixels: Vec::new() });
    }
    let count = width as usize * height as usize;
    let data = bytes.get(header..header + count * depth as usize)?;
    let mut pixels = Vec::with_capacity(count * 4);
    for p in data.chunks_exact(depth as usize) {
        let px = match depth {
            1 => [p[0], p[0], p[0], 255],
            2 => [p[0], p[0], p[0], p[1]],
            3 => [p[0], p[1], p[2], 255],
            4 => [p[0], p[1], p[2], p[3]],
            _ => return None,
        };
        pixels.extend_from_slice(&px);
    }
    Some(Rgba { width, height, pixels })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn brush(w: u32, h: u32, v: u8) -> Vec<u8> {
        let mut b = Vec::new();
        for x in [29, 2, w, h, 1] {
            b.extend_from_slice(&u32::to_be_bytes(x));
        }
        b.extend_from_slice(b"GIMP");
        b.extend_from_slice(&u32::to_be_bytes(10));
        b.push(0);
        b.extend(std::iter::repeat_n(v, (w * h) as usize));
        b
    }

    #[test]
    fn a_pipe_reads_its_cells_and_how_it_picks() {
        let mut file = b"name\n2 ncells:2 dim:1 rank0:2 sel0:random\n".to_vec();
        file.extend(brush(2, 2, 9));
        file.extend(brush(3, 1, 7));
        let pipe = gih(&file, true).unwrap();
        assert_eq!(pipe.cells.len(), 2);
        assert!(pipe.grey);
        // a grey brush's grey: the lightness of what it doesn't paint
        assert_eq!((pipe.cells[1].width, pipe.cells[1].pixels[0]), (3, 255 - 7));
        assert_eq!(mask_of(&pipe.cells[1]).pixels[0], 7);
        assert_eq!(
            (pipe.ranks.as_slice(), pipe.selections.as_slice()),
            ([2_u32].as_slice(), [Selection::Random].as_slice())
        );
    }
}
