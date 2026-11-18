//! The system's clipboard. What is cut or copied here goes on it in place of what it held, as a
//! picture other apps paste with its transparency (PNG, and on Windows CF_DIBV5 too); Paste takes
//! the pictures other apps copy once they are newer than that. Each look and each copy is on a
//! thread of its own: the owner of what is copied may take seconds to hand it over, and a big
//! canvas takes a moment to encode.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex};

use procraft_core::ThumbnailImage;

/// The system's clipboard, one connection for as long as the app runs: looks and copies take
/// turns on it, and under X11 it is what hands other apps what was copied here.
static CLIPBOARD: Mutex<Option<arboard::Clipboard>> = Mutex::new(None);

/// `then` with the system's clipboard; none if it can't be had.
fn with_clipboard<T>(then: impl FnOnce(&mut arboard::Clipboard) -> T) -> Option<T> {
    let mut held = CLIPBOARD.lock().ok()?;
    if held.is_none() {
        *held = arboard::Clipboard::new().ok();
    }
    held.as_mut().map(then)
}

/// How many copies have been made here: one waiting for its turn on the clipboard is left out
/// once a newer one is made.
static COPIES: AtomicU64 = AtomicU64::new(0);

/// What was last cut or copied here, for the studio's source editors, which paste on their own.
struct OwnCopy {
    image: Arc<ThumbnailImage>,
    /// Which copy it is ([`COPIES`]).
    turn: u64,
    /// The mark of what the system held once it was put there (none until then).
    mark: Option<u64>,
    /// Which files count as pictures.
    picture: fn(&std::path::Path) -> bool,
}
static OWN: Mutex<Option<OwnCopy>> = Mutex::new(None);

/// What was last cut or copied here, unless the system's clipboard took something else since.
pub fn own_copy_unless_replaced() -> Option<ThumbnailImage> {
    let (image, mark, picture) = OWN.lock().ok()?.as_ref().map(|own| (own.image.clone(), own.mark, own.picture))?;
    let replaced = mark.is_some_and(|mark| Held::read(picture).mark() != mark);
    (!replaced).then(|| ThumbnailImage::clone(&image))
}

/// Puts `copy` (premultiplied, its colours in the profile `icc`) on the system's clipboard in
/// place of what it held, as a picture other apps paste: PNG, which Krita, Photoshop and GIMP take
/// first, carrying the profile, then CF_DIBV5, both with the transparency. Both are made before
/// the clipboard is opened, as other apps wait while it is. Whether it went.
#[cfg(windows)]
fn put(_clipboard: &mut arboard::Clipboard, copy: &ThumbnailImage, icc: &[u8]) -> bool {
    use clipboard_win::{formats, raw};
    let straight = procraft_core::copy_straight(copy);
    let (Some(png), Some(dib)) = (procraft_core::picture_png(&straight, icc), dibv5(&straight)) else { return false };
    let Some(png_format) = clipboard_win::register_format("PNG") else { return false };
    let Ok(_open) = clipboard_win::Clipboard::new_attempts(10) else { return false };
    raw::empty().is_ok()
        && raw::set_without_clear(png_format.get(), &png).is_ok()
        && raw::set_without_clear(formats::CF_DIBV5, &dib).is_ok()
}

/// Puts `copy` (premultiplied) on the system's clipboard in place of what it held, as a picture
/// other apps paste with its transparency (PNG under X11, TIFF on a Mac; neither can carry a
/// profile here). Whether it went.
#[cfg(not(windows))]
fn put(clipboard: &mut arboard::Clipboard, copy: &ThumbnailImage, _icc: &[u8]) -> bool {
    let straight = procraft_core::copy_straight(copy);
    let (width, height) = (straight.width as usize, straight.height as usize);
    clipboard.set_image(arboard::ImageData { width, height, bytes: straight.rgba.into() }).is_ok()
}

/// A picture in straight colours as Windows' CF_DIBV5: its header, the colour masks again (where
/// Windows' own and Qt's readers look for them), then 32-bit BGRA rows from the bottom up, the
/// alpha straight. None if it is too big for one, or its pixels aren't its size.
#[cfg(any(windows, test))]
fn dibv5(straight: &ThumbnailImage) -> Option<Vec<u8>> {
    const HEADER: u32 = 124;
    const BI_BITFIELDS: u32 = 3;
    const LCS_SRGB: u32 = 0x7352_4742;
    const LCS_GM_IMAGES: u32 = 4;
    const MASKS: [u32; 4] = [0x00ff_0000, 0x0000_ff00, 0x0000_00ff, 0xff00_0000];
    let row = (straight.width as usize).checked_mul(4)?;
    let size = u32::try_from(row.checked_mul(straight.height as usize)?).ok()?;
    if size == 0 || straight.rgba.len() != size as usize {
        return None;
    }
    let (width, height) = (i32::try_from(straight.width).ok()?, i32::try_from(straight.height).ok()?);
    let mut out = Vec::with_capacity(HEADER as usize + 12 + size as usize);
    out.extend_from_slice(&HEADER.to_le_bytes());
    out.extend_from_slice(&width.to_le_bytes());
    // the height up: rows from the bottom, as every app reads them (some can't the other way)
    out.extend_from_slice(&height.to_le_bytes());
    // one plane, 32 bits a pixel, in the masks' channels
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&BI_BITFIELDS.to_le_bytes());
    out.extend_from_slice(&size.to_le_bytes());
    // 132 pixels an inch, no palette
    let resolution = i32::try_from(procraft_core::COPY_PIXELS_A_METRE).ok()?;
    out.extend_from_slice(&resolution.to_le_bytes());
    out.extend_from_slice(&resolution.to_le_bytes());
    out.extend_from_slice(&[0; 8]);
    // red, green, blue and alpha
    for mask in MASKS {
        out.extend_from_slice(&mask.to_le_bytes());
    }
    // sRGB (its end points and gammas unused), for pictures
    out.extend_from_slice(&LCS_SRGB.to_le_bytes());
    out.extend_from_slice(&[0; 48]);
    out.extend_from_slice(&LCS_GM_IMAGES.to_le_bytes());
    // no profile
    out.extend_from_slice(&[0; 12]);
    // red, green and blue again
    for mask in &MASKS[..3] {
        out.extend_from_slice(&mask.to_le_bytes());
    }
    for line in straight.rgba.chunks_exact(row).rev() {
        out.extend(line.as_chunks::<4>().0.iter().flat_map(|&[r, g, b, a]| [b, g, r, a]));
    }
    Some(out)
}

/// What the system's clipboard holds that can be pasted.
#[derive(Debug, PartialEq)]
enum Held {
    /// A picture, as straight RGBA.
    Picture(ThumbnailImage),
    /// Picture files copied in a file manager.
    Files(Vec<PathBuf>),
    Nothing,
}

impl Held {
    /// What is on the clipboard now (`picture` says which files are pictures).
    fn read(picture: fn(&std::path::Path) -> bool) -> Held {
        with_clipboard(|clipboard| Held::on(clipboard, picture)).unwrap_or(Held::Nothing)
    }

    /// What `clipboard` holds.
    fn on(clipboard: &mut arboard::Clipboard, picture: fn(&std::path::Path) -> bool) -> Held {
        if let Ok(image) = clipboard.get_image() {
            return Held::Picture(ThumbnailImage {
                width: image.width as u32,
                height: image.height as u32,
                rgba: image.bytes.into_owned(),
            });
        }
        let files = clipboard.get().file_list().unwrap_or_default();
        let files: Vec<PathBuf> = files.into_iter().filter(|f| picture(f)).collect();
        if files.is_empty() { Held::Nothing } else { Held::Files(files) }
    }

    /// What tells this from what the clipboard held before.
    fn mark(&self) -> u64 {
        let mut hash = DefaultHasher::new();
        match self {
            Held::Picture(image) => (image.width, image.height, &image.rgba).hash(&mut hash),
            Held::Files(files) => files.hash(&mut hash),
            Held::Nothing => return 0,
        }
        hash.finish()
    }
}

/// What Paste came to.
#[derive(Debug, PartialEq)]
pub enum Pasted {
    /// What was copied here.
    Own { private: bool },
    /// A picture from another app, as straight RGBA.
    Picture { image: ThumbnailImage, private: bool },
    /// Picture files from a file manager.
    Files { paths: Vec<PathBuf>, private: bool },
}

/// What Paste takes, given what the system holds and the mark of what it held when something was
/// last copied here (none: nothing has been): that, unchanged, is older than the app's own copy.
fn pasted(held: Held, before_ours: Option<u64>, private: bool) -> Pasted {
    if before_ours == Some(held.mark()) {
        return Pasted::Own { private };
    }
    match held {
        Held::Picture(image) => Pasted::Picture { image, private },
        Held::Files(paths) => Pasted::Files { paths, private },
        Held::Nothing => Pasted::Own { private },
    }
}

/// The system's clipboard as Paste sees it.
pub struct SystemClipboard {
    /// Which files count as pictures.
    picture: fn(&std::path::Path) -> bool,
    /// The mark of what it held as something was last copied here, and that look while under way.
    before_ours: Option<u64>,
    marking: Option<Receiver<u64>>,
    /// It held something to paste when last looked at for Paste's button, and that look.
    pub offers: bool,
    offering: Option<Receiver<bool>>,
    /// A paste waiting to see what it holds (no look: one asked for as a copy made here went on
    /// it, that copy being the newest), and whether it goes on a private layer.
    pasting: Option<(Option<Receiver<Held>>, bool)>,
}

impl SystemClipboard {
    pub fn new(picture: fn(&std::path::Path) -> bool) -> SystemClipboard {
        SystemClipboard { picture, before_ours: None, marking: None, offers: false, offering: None, pasting: None }
    }

    /// Reads it on a thread, what `then` makes of that coming to the receiver.
    fn look<T: Send + 'static>(&self, then: impl FnOnce(Held) -> T + Send + 'static) -> Receiver<T> {
        let (done, result) = channel();
        let picture = self.picture;
        std::thread::spawn(move || {
            // (the app may be gone by then)
            let _ = done.send(then(Held::read(picture)));
        });
        result
    }

    /// `copy` (premultiplied, its colours in the profile `icc`) was copied here: it goes on the
    /// system's clipboard in place of what that held, as a picture other apps paste, on a thread
    /// of its own (here it is kept as it is, for Paste). What the system holds once it is there is
    /// marked, to tell it from what another app copies later.
    pub fn copied_here(&mut self, copy: &ThumbnailImage, icc: Vec<u8>) {
        let turn = COPIES.fetch_add(1, Ordering::SeqCst) + 1;
        let copy = Arc::new(copy.clone());
        let picture = self.picture;
        if let Ok(mut own) = OWN.lock() {
            *own = Some(OwnCopy { image: copy.clone(), turn, mark: None, picture });
        }
        let (done, marked) = channel();
        std::thread::spawn(move || {
            let mark = with_clipboard(|clipboard| {
                // a newer copy goes on it in this one's place
                if COPIES.load(Ordering::SeqCst) != turn {
                    return None;
                }
                if !put(clipboard, &copy, &icc) {
                    log::warn!("a copy couldn't go on the system's clipboard");
                }
                Some(Held::on(clipboard, picture).mark())
            });
            let mark = match mark {
                Some(Some(mark)) => mark,
                Some(None) => return,
                None => Held::Nothing.mark(),
            };
            if let Ok(mut own) = OWN.lock()
                && let Some(own) = own.as_mut().filter(|own| own.turn == turn)
            {
                own.mark.get_or_insert(mark);
            }
            // (the app may be gone by then)
            let _ = done.send(mark);
        });
        self.marking = Some(marked);
    }

    /// Paste's button is coming up: whether it is lit goes by this too.
    pub fn offer(&mut self) {
        self.offering = Some(self.look(|held| held != Held::Nothing));
    }

    /// Paste was asked for (on a private layer, if `private`).
    pub fn paste(&mut self, private: bool) {
        // a copy made here going on it now is the newest there is: no need to look
        let look = self.marking.is_none().then(|| self.look(|held| held));
        self.pasting = Some((look, private));
    }

    /// What Paste came to, once it has. Called every frame.
    pub fn take_pasted(&mut self) -> Option<Pasted> {
        if let Some(offering) = &self.offering {
            match offering.try_recv() {
                Ok(offers) => {
                    self.offers = offers;
                    self.offering = None;
                }
                Err(TryRecvError::Disconnected) => self.offering = None,
                Err(TryRecvError::Empty) => {}
            }
        }
        if let Some((None, private)) = self.pasting {
            self.pasting = None;
            return Some(Pasted::Own { private });
        }
        if let Some(marking) = &self.marking {
            match marking.try_recv() {
                Ok(mark) => {
                    self.before_ours = Some(mark);
                    self.marking = None;
                }
                Err(TryRecvError::Disconnected) => self.marking = None,
                // what was copied here is yet to be told from what is there
                Err(TryRecvError::Empty) => return None,
            }
        }
        let (Some(pasting), private) = self.pasting.as_ref()? else { return None };
        let private = *private;
        match pasting.try_recv() {
            Ok(held) => {
                self.pasting = None;
                Some(pasted(held, self.before_ours, private))
            }
            Err(TryRecvError::Disconnected) => {
                self.pasting = None;
                Some(Pasted::Own { private })
            }
            Err(TryRecvError::Empty) => None,
        }
    }

    /// A look is under way, so frames are to keep coming.
    pub fn busy(&self) -> bool {
        self.offering.is_some() || self.marking.is_some() || self.pasting.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(fill: u8) -> ThumbnailImage {
        ThumbnailImage { width: 2, height: 1, rgba: vec![fill; 8] }
    }

    #[test]
    fn paste_takes_what_another_app_copied_unless_something_was_copied_here_since() {
        // nothing copied here: the system's picture
        let got = pasted(Held::Picture(picture(1)), None, false);
        assert_eq!(got, Pasted::Picture { image: picture(1), private: false });
        // copied here with that on the system's: ours is the newer
        let before = Some(Held::Picture(picture(1)).mark());
        assert_eq!(pasted(Held::Picture(picture(1)), before, true), Pasted::Own { private: true });
        // then copied in another app: that is
        let got = pasted(Held::Picture(picture(2)), before, true);
        assert_eq!(got, Pasted::Picture { image: picture(2), private: true });
        // nothing there, or nothing that can be pasted
        assert_eq!(pasted(Held::Nothing, None, false), Pasted::Own { private: false });
        assert_eq!(pasted(Held::Nothing, before, false), Pasted::Own { private: false });
    }

    #[test]
    fn picture_files_copied_elsewhere_are_pasted_as_files() {
        let files = vec![PathBuf::from("/a/b.png")];
        let got = pasted(Held::Files(files.clone()), Some(Held::Nothing.mark()), false);
        assert_eq!(got, Pasted::Files { paths: files.clone(), private: false });
        let before = Some(Held::Files(files.clone()).mark());
        assert_eq!(pasted(Held::Files(files), before, false), Pasted::Own { private: false });
    }

    #[test]
    fn a_paste_asked_for_as_a_copy_goes_on_the_clipboard_takes_that_copy() {
        let mut clipboard = SystemClipboard::new(|_| false);
        // the copy made here still on its way (the clipboard isn't touched)
        let (_putting, marked) = channel();
        clipboard.marking = Some(marked);
        clipboard.paste(true);
        assert!(matches!(clipboard.pasting, Some((None, true))));
        assert_eq!(clipboard.take_pasted(), Some(Pasted::Own { private: true }));
        assert_eq!(clipboard.take_pasted(), None);
        assert!(clipboard.busy());
    }

    #[test]
    fn a_picture_as_a_dibv5_reads_back_with_its_transparency() {
        // 3 × 2, rows top to bottom: solid, half, clear and faint pixels
        let rgba: Vec<u8> =
            [[255, 0, 0, 255], [0, 255, 0, 128], [0, 0, 255, 0], [10, 20, 30, 40], [255, 255, 255, 255], [1, 2, 3, 1]]
                .concat();
        let picture = ThumbnailImage { width: 3, height: 2, rgba: rgba.clone() };
        let dib = dibv5(&picture).unwrap();
        assert_eq!(dib.len(), 124 + 12 + 3 * 2 * 4);
        // after the header the masks again, then the bottom row first, blue green red alpha
        assert_eq!(&dib[124..136], &[0, 0, 255, 0, 0, 255, 0, 0, 255, 0, 0, 0]);
        // 132 pixels an inch, across and down
        assert_eq!(&dib[24..32], &[0x4d, 0x14, 0, 0, 0x4d, 0x14, 0, 0]);
        assert_eq!(&dib[136..140], &[30, 20, 10, 40]);
        // read as other apps read it
        let decoder = image::codecs::bmp::BmpDecoder::new_without_file_header(std::io::Cursor::new(&dib)).unwrap();
        let read = image::DynamicImage::from_decoder(decoder).unwrap().into_rgba8();
        assert_eq!(read.dimensions(), (3, 2));
        assert_eq!(read.into_raw(), rgba);
        // its PNG too, and a copy's colours brought out of their alpha
        let copy = ThumbnailImage { width: 1, height: 1, rgba: vec![50, 25, 0, 128] };
        let straight = procraft_core::copy_straight(&copy);
        assert_eq!(straight.rgba, vec![100, 50, 0, 128]);
        let png = procraft_core::picture_png(&straight, &[]).unwrap();
        assert_eq!(image::load_from_memory(&png).unwrap().into_rgba8().into_raw(), straight.rgba);
    }

    #[test]
    fn a_picture_that_isnt_its_size_makes_no_dibv5() {
        assert!(dibv5(&ThumbnailImage { width: 2, height: 2, rgba: vec![0; 12] }).is_none());
        assert!(dibv5(&ThumbnailImage { width: 0, height: 5, rgba: vec![] }).is_none());
        assert!(dibv5(&ThumbnailImage { width: u32::MAX, height: u32::MAX, rgba: vec![0; 4] }).is_none());
    }

    #[test]
    fn a_picture_of_another_size_with_the_same_bytes_is_another_picture() {
        let wide = Held::Picture(ThumbnailImage { width: 2, height: 1, rgba: vec![7; 8] });
        let tall = Held::Picture(ThumbnailImage { width: 1, height: 2, rgba: vec![7; 8] });
        assert_ne!(wide.mark(), tall.mark());
        assert_eq!(Held::Nothing.mark(), 0);
    }
}
