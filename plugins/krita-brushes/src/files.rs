//! The file formats a Krita brush comes in: PNG (a preset is one, its settings in a text chunk;
//! tips and patterns are too), zip (a bundle), base64 (pictures carried inside a preset). In the
//! app, the app's utilities unpack and decode (`utilities.rs`); elsewhere, the code here does.

/// An image as RGBA, 8 bits a channel, straight, rows from the top.
#[derive(Clone, Debug, PartialEq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// The largest side of a picture taken, and the most bytes one may unpack to.
#[cfg(not(target_arch = "wasm32"))]
const MOST_SIDE: u32 = 8192;
const MOST_UNPACKED: usize = 64 << 20;

/// `data` unpacked: zlib, or raw deflate.
fn unpack(data: &[u8], zlib: bool) -> Option<Vec<u8>> {
    #[cfg(target_arch = "wasm32")]
    let unpacked = crate::utilities::inflate(data, zlib);
    #[cfg(not(target_arch = "wasm32"))]
    let unpacked = {
        use miniz_oxide::inflate::{decompress_to_vec_with_limit, decompress_to_vec_zlib_with_limit};
        if zlib {
            decompress_to_vec_zlib_with_limit(data, MOST_UNPACKED).ok()
        } else {
            decompress_to_vec_with_limit(data, MOST_UNPACKED).ok()
        }
    };
    unpacked.filter(|d| d.len() <= MOST_UNPACKED)
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(*b.get(at..)?.first_chunk::<4>()?))
}

/// The chunks of a PNG: kind and data.
fn chunks(png: &[u8]) -> impl Iterator<Item = (&[u8], &[u8])> {
    let mut at = if png.starts_with(b"\x89PNG\r\n\x1a\n") { 8 } else { usize::MAX };
    core::iter::from_fn(move || {
        let len = be32(png, at)? as usize;
        let kind = png.get(at + 4..at + 8)?;
        let data = png.get(at + 8..at + 8 + len)?;
        at += 12 + len;
        Some((kind, data))
    })
}

/// The text a PNG holds, by keyword (tEXt, zTXt and iTXt).
pub fn png_texts(png: &[u8]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (kind, data) in chunks(png) {
        let Some(zero) = data.iter().position(|&b| b == 0) else { continue };
        let key = String::from_utf8_lossy(&data[..zero]).into_owned();
        let rest = &data[zero + 1..];
        let text = match kind {
            b"tEXt" => Some(rest.to_vec()),
            b"zTXt" => rest.get(1..).and_then(|z| unpack(z, true)),
            b"iTXt" => (|| {
                let (&compressed, rest) = rest.split_first()?;
                let rest = rest.get(1..)?;
                let lang = rest.iter().position(|&b| b == 0)?;
                let rest = &rest[lang + 1..];
                let translated = rest.iter().position(|&b| b == 0)?;
                let body = &rest[translated + 1..];
                if compressed != 0 { unpack(body, true) } else { Some(body.to_vec()) }
            })(),
            _ => None,
        };
        if let Some(text) = text {
            out.push((key, String::from_utf8_lossy(&text).into_owned()));
        }
    }
    out
}

/// The PNG with only what draws its picture: its header, palette, transparency and image data,
/// none of its text.
pub fn png_picture_only(png: &[u8]) -> Option<Vec<u8>> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut any = false;
    let mut at = 8;
    while let Some(len) = be32(png, at) {
        let whole = png.get(at..at + 12 + len as usize)?;
        let kind = whole.get(4..8)?;
        if matches!(kind, b"IHDR" | b"PLTE" | b"tRNS" | b"IDAT" | b"IEND") {
            out.extend_from_slice(whole);
            any |= kind == b"IDAT";
        }
        at += 12 + len as usize;
        if kind == b"IEND" {
            break;
        }
    }
    any.then_some(out)
}

/// A PNG's width and height, from its header.
pub fn png_size(png: &[u8]) -> Option<(u32, u32)> {
    let (kind, header) = chunks(png).next()?;
    let (w, h) = (be32(header, 0)?, be32(header, 4)?);
    (kind == b"IHDR" && w > 0 && h > 0).then_some((w, h))
}

/// A PNG decoded: any colour type and depth, interlaced or not.
#[cfg(target_arch = "wasm32")]
pub fn png_decode(png: &[u8]) -> Option<Rgba> {
    crate::utilities::decode_image(png)
}

/// A PNG decoded: any colour type and depth, interlaced or not.
#[cfg(not(target_arch = "wasm32"))]
pub fn png_decode(png: &[u8]) -> Option<Rgba> {
    let mut header = None;
    let mut palette: &[u8] = &[];
    let mut alpha: &[u8] = &[];
    let mut packed = Vec::new();
    for (kind, data) in chunks(png) {
        match kind {
            b"IHDR" => header = Some(data),
            b"PLTE" => palette = data,
            b"tRNS" => alpha = data,
            b"IDAT" => packed.extend_from_slice(data),
            b"IEND" => break,
            _ => {}
        }
    }
    let h = header?;
    let (width, height) = (be32(h, 0)?, be32(h, 4)?);
    let (depth, colour, interlace) = (*h.get(8)?, *h.get(9)?, *h.get(12)?);
    if width == 0 || height == 0 || width > MOST_SIDE || height > MOST_SIDE {
        return None;
    }
    let channels = match colour {
        0 | 3 => 1,
        2 => 3,
        4 => 2,
        6 => 4,
        _ => return None,
    };
    if !matches!(depth, 1 | 2 | 4 | 8 | 16) || (depth > 8 && colour == 3) || (depth < 8 && !matches!(colour, 0 | 3)) {
        return None;
    }
    let raw = unpack(&packed, true)?;
    let bits = channels * depth as usize;
    let mut pixels = vec![0_u8; width as usize * height as usize * 4];
    let format = Format { channels, depth, colour, palette, alpha };
    if interlace == 1 {
        // seven passes over every eighth row and column, starting where each says
        const PASSES: [(u32, u32, u32, u32); 7] =
            [(0, 0, 8, 8), (4, 0, 8, 8), (0, 4, 4, 8), (2, 0, 4, 4), (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2)];
        let mut at = 0;
        for (x0, y0, dx, dy) in PASSES {
            let (w, hh) = ((width.saturating_sub(x0)).div_ceil(dx), (height.saturating_sub(y0)).div_ceil(dy));
            if w == 0 || hh == 0 {
                continue;
            }
            let stride = (w as usize * bits).div_ceil(8);
            let rows = unfilter(raw.get(at..)?, stride, hh as usize, bits.div_ceil(8))?;
            at += (stride + 1) * hh as usize;
            for y in 0..hh {
                for x in 0..w {
                    let row = rows.get(y as usize * stride..(y as usize + 1) * stride)?;
                    let px = format.pixel(row, x as usize);
                    let (cx, cy) = (x0 + x * dx, y0 + y * dy);
                    let o = (cy as usize * width as usize + cx as usize) * 4;
                    pixels.get_mut(o..o + 4)?.copy_from_slice(&px);
                }
            }
        }
    } else {
        let stride = (width as usize * bits).div_ceil(8);
        let rows = unfilter(&raw, stride, height as usize, bits.div_ceil(8))?;
        for y in 0..height as usize {
            let row = rows.get(y * stride..(y + 1) * stride)?;
            for x in 0..width as usize {
                let o = (y * width as usize + x) * 4;
                pixels.get_mut(o..o + 4)?.copy_from_slice(&format.pixel(row, x));
            }
        }
    }
    Some(Rgba { width, height, pixels })
}

#[cfg(not(target_arch = "wasm32"))]
struct Format<'a> {
    channels: usize,
    depth: u8,
    colour: u8,
    palette: &'a [u8],
    alpha: &'a [u8],
}

#[cfg(not(target_arch = "wasm32"))]
impl Format<'_> {
    /// Sample `i` (a channel) of pixel `x` in `row`, as 8 bits (palette indices as they are).
    fn sample(&self, row: &[u8], x: usize, i: usize) -> u8 {
        let n = x * self.channels + i;
        match self.depth {
            16 => row.get(n * 2).copied().unwrap_or(0),
            8 => row.get(n).copied().unwrap_or(0),
            d => {
                let d = d as usize;
                let bit = n * d;
                let byte = row.get(bit / 8).copied().unwrap_or(0);
                let v = (byte >> (8 - d - bit % 8)) & ((1 << d) - 1) as u8;
                if self.colour == 3 { v } else { (u32::from(v) * 255 / ((1 << d) - 1)) as u8 }
            }
        }
    }

    fn pixel(&self, row: &[u8], x: usize) -> [u8; 4] {
        let s = |i| self.sample(row, x, i);
        match self.colour {
            0 => {
                let g = s(0);
                // a grey given as see-through
                let clear = self.alpha.len() >= 2 && self.depth <= 8 && u16::from(g) == self.grey_key();
                [g, g, g, if clear { 0 } else { 255 }]
            }
            2 => [s(0), s(1), s(2), 255],
            3 => {
                let i = s(0) as usize;
                let rgb = self.palette.get(i * 3..i * 3 + 3).unwrap_or(&[0, 0, 0]);
                [rgb[0], rgb[1], rgb[2], self.alpha.get(i).copied().unwrap_or(255)]
            }
            4 => {
                let g = s(0);
                [g, g, g, s(1)]
            }
            _ => [s(0), s(1), s(2), s(3)],
        }
    }

    fn grey_key(&self) -> u16 {
        let key = u16::from_be_bytes([self.alpha[0], self.alpha[1]]);
        if self.depth < 8 { key * 255 / ((1 << self.depth) - 1) } else { key }
    }
}

/// Rows of `stride` bytes undone from PNG's filters (each row its filter byte first).
#[cfg(not(target_arch = "wasm32"))]
fn unfilter(raw: &[u8], stride: usize, rows: usize, bpp: usize) -> Option<Vec<u8>> {
    let mut out = vec![0_u8; stride * rows];
    for y in 0..rows {
        let line = raw.get(y * (stride + 1)..(y + 1) * (stride + 1))?;
        let (&filter, data) = line.split_first()?;
        let (before, now) = out.split_at_mut(y * stride);
        let up = if y > 0 { before.get((y - 1) * stride..) } else { None };
        let now = now.get_mut(..stride)?;
        for x in 0..stride {
            let a = if x >= bpp { now[x - bpp] } else { 0 };
            let b = up.map_or(0, |u| u[x]);
            let c = if x >= bpp { up.map_or(0, |u| u[x - bpp]) } else { 0 };
            let v = data[x];
            now[x] = match filter {
                0 => v,
                1 => v.wrapping_add(a),
                2 => v.wrapping_add(b),
                3 => v.wrapping_add(((u16::from(a) + u16::from(b)) / 2) as u8),
                4 => {
                    let p = i16::from(a) + i16::from(b) - i16::from(c);
                    let (pa, pb, pc) = ((p - i16::from(a)).abs(), (p - i16::from(b)).abs(), (p - i16::from(c)).abs());
                    let pred = if pa <= pb && pa <= pc {
                        a
                    } else if pb <= pc {
                        b
                    } else {
                        c
                    };
                    v.wrapping_add(pred)
                }
                _ => return None,
            };
        }
    }
    Some(out)
}

/// The files of a zip, by name: stored or deflated, unpacked as they are asked for.
pub struct Zip<'a> {
    bytes: &'a [u8],
    entries: Vec<(String, usize, u16, usize, usize)>,
}

fn le16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(*b.get(at..)?.first_chunk::<2>()?))
}

fn le32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(*b.get(at..)?.first_chunk::<4>()?))
}

impl<'a> Zip<'a> {
    pub fn read(bytes: &'a [u8]) -> Option<Zip<'a>> {
        if !bytes.starts_with(b"PK\x03\x04") {
            return None;
        }
        // the end record, a comment after it at most 64 KiB
        let from = bytes.len().saturating_sub(22 + 0xffff);
        let end =
            (from..=bytes.len().saturating_sub(22)).rev().find(|&i| bytes.get(i..i + 4) == Some(b"PK\x05\x06"))?;
        let count = le16(bytes, end + 10)? as usize;
        let mut at = le32(bytes, end + 16)? as usize;
        let mut entries = Vec::with_capacity(count.min(4096));
        for _ in 0..count {
            if bytes.get(at..at + 4)? != b"PK\x01\x02" {
                return None;
            }
            let method = le16(bytes, at + 10)?;
            let packed = le32(bytes, at + 20)? as usize;
            let size = le32(bytes, at + 24)? as usize;
            let (name_len, extra, comment) =
                (le16(bytes, at + 28)? as usize, le16(bytes, at + 30)? as usize, le16(bytes, at + 32)? as usize);
            let local = le32(bytes, at + 42)? as usize;
            let name = String::from_utf8_lossy(bytes.get(at + 46..at + 46 + name_len)?).into_owned();
            entries.push((name, local, method, packed, size));
            at += 46 + name_len + extra + comment;
        }
        Some(Zip { bytes, entries })
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|e| e.0.as_str())
    }

    /// The file called `name`, unpacked.
    pub fn file(&self, name: &str) -> Option<Vec<u8>> {
        let &(_, local, method, packed, size) = self.entries.iter().find(|e| e.0 == name)?;
        let b = self.bytes;
        if b.get(local..local + 4)? != b"PK\x03\x04" || size > MOST_UNPACKED {
            return None;
        }
        let start = local + 30 + le16(b, local + 26)? as usize + le16(b, local + 28)? as usize;
        let data = b.get(start..start + packed)?;
        match method {
            0 => Some(data.to_vec()),
            8 => unpack(data, false),
            _ => None,
        }
    }
}

/// What each byte is worth in base64: 64 for white space, 65 for its end, 66 for anything else.
const BASE64: [u8; 256] = {
    let mut table = [66_u8; 256];
    let mut i = 0;
    while i < 26 {
        table[b'A' as usize + i] = i as u8;
        table[b'a' as usize + i] = 26 + i as u8;
        i += 1;
    }
    let mut d = 0;
    while d < 10 {
        table[b'0' as usize + d] = 52 + d as u8;
        d += 1;
    }
    table[b'+' as usize] = 62;
    table[b'-' as usize] = 62;
    table[b'/' as usize] = 63;
    table[b'_' as usize] = 63;
    table[b'=' as usize] = 65;
    let mut w = 0;
    let space = [b' ', b'\t', b'\n', b'\r', 0x0c];
    while w < space.len() {
        table[space[w] as usize] = 64;
        w += 1;
    }
    table
};

/// A picture a setting carries in base64: Krita writes a pattern's file as base64 text, which
/// its settings then write as base64 again, so it is read through twice where once leaves text.
pub fn carried_picture(text: &str) -> Option<Vec<u8>> {
    let picture = |b: &[u8]| b.starts_with(b"\x89PNG") || b.get(20..24) == Some(b"GPAT");
    let once = base64(text).filter(|b| !b.is_empty())?;
    if picture(&once) {
        return Some(once);
    }
    std::str::from_utf8(&once).ok().and_then(base64).filter(|b| picture(b)).or(Some(once))
}

/// `text` as base64, white space passed over; none if it isn't.
pub fn base64(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3 + 3);
    // four letters at a time while they are all letters, as they mostly are
    let mut at = 0;
    while let Some(four) = bytes.get(at..at + 4) {
        let v =
            [BASE64[four[0] as usize], BASE64[four[1] as usize], BASE64[four[2] as usize], BASE64[four[3] as usize]];
        if (v[0] | v[1] | v[2] | v[3]) >= 64 {
            break;
        }
        let n = (u32::from(v[0]) << 18) | (u32::from(v[1]) << 12) | (u32::from(v[2]) << 6) | u32::from(v[3]);
        out.extend_from_slice(&[(n >> 16) as u8, (n >> 8) as u8, n as u8]);
        at += 4;
    }
    let (mut acc, mut bits) = (0_u32, 0);
    for &c in &bytes[at..] {
        let v = match BASE64[c as usize] {
            64 => continue,
            65 => break,
            66 => return None,
            v => v,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_reads_padded_text() {
        assert_eq!(base64("aGVs\nbG8=").unwrap(), b"hello");
        assert!(base64("a*b").is_none());
    }

    #[test]
    fn unfilter_undoes_sub_and_up() {
        // two rows of two grey pixels: [1, 2] raw, then up-filtered +1 each
        let raw = [1, 1, 1, 2, 1, 1];
        assert_eq!(unfilter(&raw, 2, 2, 1).unwrap(), [1, 2, 2, 3]);
    }
}
