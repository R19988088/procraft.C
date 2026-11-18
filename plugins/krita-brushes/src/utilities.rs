//! The app's utilities (its guide to plugins): unpacking and decoding done by the app, many times
//! faster than this module's own code runs in it. Built for anything else (its tests), the plugin
//! does the same itself (see `files.rs`).

#[cfg(target_arch = "wasm32")]
mod app {
    #[link(wasm_import_module = "procraft")]
    unsafe extern "C" {
        pub fn inflate(from: i32, len: i32, kind: i32) -> i32;
        pub fn decode_image(from: i32, len: i32) -> i64;
        pub fn read_answer(to: i32) -> i32;
    }
}

/// The most bytes it takes back from one: past it the plugin's memory would run out.
#[cfg(target_arch = "wasm32")]
const MOST: usize = 64 << 20;

/// What the app made, `len` bytes, read into the plugin's memory.
#[cfg(target_arch = "wasm32")]
fn answer(len: usize) -> Option<Vec<u8>> {
    if len > MOST {
        return None;
    }
    let mut out = vec![0_u8; len];
    (unsafe { app::read_answer(out.as_mut_ptr() as i32) } == 1).then_some(out)
}

/// `bytes` unpacked, zlib or raw deflate.
#[cfg(target_arch = "wasm32")]
pub fn inflate(bytes: &[u8], zlib: bool) -> Option<Vec<u8>> {
    let len = unsafe { app::inflate(bytes.as_ptr() as i32, bytes.len() as i32, i32::from(zlib)) };
    answer(usize::try_from(len).ok()?)
}

/// A PNG decoded, as RGBA.
#[cfg(target_arch = "wasm32")]
pub fn decode_image(bytes: &[u8]) -> Option<crate::files::Rgba> {
    let size = unsafe { app::decode_image(bytes.as_ptr() as i32, bytes.len() as i32) } as u64;
    let (width, height) = (size as u32, (size >> 32) as u32);
    if width == 0 || height == 0 {
        return None;
    }
    let pixels = answer((width as usize).checked_mul(height as usize)?.checked_mul(4)?)?;
    Some(crate::files::Rgba { width, height, pixels })
}
