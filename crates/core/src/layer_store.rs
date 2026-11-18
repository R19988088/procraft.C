//! A layer's store (what a brush keeps with a layer between strokes, see the canvas's
//! `LayerStore`) as an artwork keeps it: `layers/<id>.store`, beside the layer's pixels.
//!
//! The file, little-endian: `PCSTORE\0`, its version (u32, [`VERSION`]), the store's kind (u32), the
//! canvas's width and height (u32 each), the hash of the layer's pixels it was saved with (u64, see
//! `project::pixels_hash`), how many planes (u32), then each plane: its length (u32) and a PNG of
//! its premultiplied RGBA8 pixels. Written whole beside its name and put in place with the rest of
//! a save. Read leniently: a file that doesn't hold up (cut short, of a later version, another size
//! or kind, a plane that can't be read) or that was saved with other pixels than the layer has now
//! (a save cut short between the two) is no store, and the layer's paint has dried as it shows.

use procraft_engine::document::LayerId;

use crate::MobileError;

const MAGIC: &[u8; 8] = b"PCSTORE\0";
pub(crate) const VERSION: u32 = 1;

/// A store read from its file: its kind, the hash of the layer's pixels it goes with, and its
/// planes' pixels.
#[derive(Debug, PartialEq)]
pub(crate) struct OpenedStore {
    pub kind: u32,
    pub layer_hash: u64,
    pub planes: Vec<Vec<u8>>,
}

/// The file of a store of `kind` on a canvas `width` × `height` whose layer's pixels hash to
/// `layer_hash`, its planes' pixels `planes` (premultiplied RGBA8, the canvas's size each).
pub(crate) fn encode(
    kind: u32,
    (width, height): (u32, u32),
    layer_hash: u64,
    planes: &[Vec<u8>],
) -> Result<Vec<u8>, MobileError> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    for v in [VERSION, kind, width, height] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&layer_hash.to_le_bytes());
    out.extend_from_slice(&u32::try_from(planes.len()).unwrap_or(u32::MAX).to_le_bytes());
    for plane in planes {
        let png = crate::project::png_of(width, height, plane)?;
        let len = u32::try_from(png.len()).map_err(|e| crate::project::file_error("saving a layer's store", e))?;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&png);
    }
    Ok(out)
}

/// The store `bytes` hold for a canvas `width` × `height`: none if they don't hold up (see the
/// module) or keep a kind of store there isn't.
pub(crate) fn decode(bytes: &[u8], (width, height): (u32, u32)) -> Option<OpenedStore> {
    let mut at = bytes.strip_prefix(MAGIC.as_slice())?;
    let mut word = || -> Option<u32> {
        let (w, rest) = at.split_first_chunk::<4>()?;
        at = rest;
        Some(u32::from_le_bytes(*w))
    };
    let (version, kind, w, h) = (word()?, word()?, word()?, word()?);
    if version != VERSION || (w, h) != (width, height) {
        return None;
    }
    let count = procraft_gpu::LayerStore::planes_of(kind);
    let (hash, rest) = at.split_first_chunk::<8>()?;
    at = rest;
    let layer_hash = u64::from_le_bytes(*hash);
    let mut word = || -> Option<u32> {
        let (w, rest) = at.split_first_chunk::<4>()?;
        at = rest;
        Some(u32::from_le_bytes(*w))
    };
    if count == 0 || word()? as usize != count {
        return None;
    }
    let mut planes = Vec::with_capacity(count);
    for _ in 0..count {
        let (len, rest) = at.split_first_chunk::<4>()?;
        let len = u32::from_le_bytes(*len) as usize;
        let png = rest.get(..len)?;
        at = rest.get(len..)?;
        planes.push(crate::project::png_pixels(png, width, height).ok()?);
    }
    Some(OpenedStore { kind, layer_hash, planes })
}

/// Where layer `id`'s store is kept in the artwork's folder `dir`.
pub(crate) fn store_path(dir: &std::path::Path, id: LayerId) -> std::path::PathBuf {
    dir.join("layers").join(format!("{}.store", id.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CanvasSession;
    use procraft_engine::view::View;

    /// A session `side` pixels a side shown on a GPU, as the app shows one.
    fn shown(s: &CanvasSession, gpu: &procraft_gpu::Gpu, side: u32) {
        let mut st = s.shared.lock();
        st.window_size = (side, side);
        st.view = Some(View::fit((side, side), (side, side)));
        st.gpu = Some(gpu.clone());
    }

    /// The store of layer `id` on the canvas of `s` once what is queued is done, with its planes'
    /// pixels.
    fn store_of(s: &CanvasSession, id: LayerId) -> Option<(u32, Vec<Vec<u8>>)> {
        let mut guard = s.shared.lock();
        let st = &mut *guard;
        let (canvas, gpu) = (st.canvas.as_mut()?, st.gpu.as_ref()?);
        canvas.render(gpu, &mut st.doc, None);
        let store = canvas.store(id)?.clone();
        Some((store.kind, store.planes.iter().map(|p| canvas.read_layer(gpu, *p).unwrap()).collect()))
    }

    #[test]
    fn a_layer_s_store_is_saved_with_it_and_opened_again_while_its_pixels_are_as_saved() {
        let Ok(gpu) = procraft_gpu::Gpu::headless() else { return };
        let side = 32;
        let s = CanvasSession::new(side, side);
        shown(&s, &gpu, side);
        s.shared.lock().ensure_canvas();
        let layer = s.shared.lock().doc.current();
        let size = (side * side * 4) as usize;
        let planes = vec![(0..size).map(|i| (i % 251) as u8).collect::<Vec<u8>>(), vec![90; size]];
        {
            let mut guard = s.shared.lock();
            let st = &mut *guard;
            let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
            assert!(canvas.put_store(gpu, layer, procraft_gpu::STORE_LIT, &planes));
        }
        let dir = std::env::temp_dir().join(format!("procraft-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let folder = dir.to_string_lossy().into_owned();
        s.save(folder.clone()).unwrap();
        assert!(store_path(&dir, layer).exists());
        // opened, as the render thread makes the canvas and as the app's own thread does: as it was
        for threaded in [true, false] {
            let opened = CanvasSession::open(folder.clone()).unwrap();
            shown(&opened, &gpu, side);
            if threaded {
                opened.shared.make_canvas();
            } else {
                opened.shared.lock().ensure_canvas();
            }
            assert_eq!(store_of(&opened, layer), Some((procraft_gpu::STORE_LIT, planes.clone())), "{threaded}");
        }
        // saved again unchanged, its file stays as it is
        let before = std::fs::metadata(store_path(&dir, layer)).and_then(|m| m.modified()).unwrap();
        s.save(folder.clone()).unwrap();
        assert_eq!(std::fs::metadata(store_path(&dir, layer)).and_then(|m| m.modified()).unwrap(), before);
        // the layer's file changed since the store was saved with it: opened without
        let layer_file = dir.join("layers").join(format!("{}.png", layer.0));
        let other: Vec<u8> = vec![255; size];
        crate::project::write_png(&layer_file, side, side, &other, &[]).unwrap();
        let opened = CanvasSession::open(folder.clone()).unwrap();
        shown(&opened, &gpu, side);
        opened.shared.lock().ensure_canvas();
        assert_eq!(store_of(&opened, layer), None);
        // cleared, the paint dries: the next save leaves no store, and undone it comes back
        s.clear_layer();
        assert_eq!(store_of(&s, layer), None);
        s.save(folder.clone()).unwrap();
        assert!(!store_path(&dir, layer).exists());
        s.undo();
        assert_eq!(store_of(&s, layer).map(|(k, _)| k), Some(procraft_gpu::STORE_LIT));
        s.save(folder).unwrap();
        assert!(store_path(&dir, layer).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_store_comes_back_as_it_was_kept_and_none_that_does_not_hold_up() {
        let (w, h) = (3, 2);
        let planes = vec![(0..24).collect::<Vec<u8>>(), vec![7; 24]];
        let bytes = encode(1, (w, h), 0xfeed, &planes).unwrap();
        let back = decode(&bytes, (w, h)).unwrap();
        assert_eq!(back, OpenedStore { kind: 1, layer_hash: 0xfeed, planes: planes.clone() });
        // another size, a kind there isn't, a later version, cut short: none
        assert!(decode(&bytes, (2, 3)).is_none());
        assert!(decode(&encode(9, (w, h), 1, &planes).unwrap(), (w, h)).is_none());
        let mut later = bytes.clone();
        later[8] = 2;
        assert!(decode(&later, (w, h)).is_none());
        assert!(decode(&bytes[..bytes.len() - 5], (w, h)).is_none());
        assert!(decode(b"PNG", (w, h)).is_none());
        // a plane of another size than the canvas
        let mut small = encode(1, (1, 1), 1, &[vec![1; 4], vec![2; 4]]).unwrap();
        small[16..20].copy_from_slice(&w.to_le_bytes());
        small[20..24].copy_from_slice(&h.to_le_bytes());
        assert!(decode(&small, (w, h)).is_none());
    }
}
