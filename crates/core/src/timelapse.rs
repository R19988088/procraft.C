//! An artwork's time-lapse: a picture of the whole canvas after each change, kept as numbered
//! JPEG frames in the artwork's folder (`timelapse/frame-000000.jpg` on), replayed or made into a
//! video at [`TIME_LAPSE_FPS`] frames a second. The video itself is made by the app, with the
//! system's encoder; this keeps the frames, the settings and whether it records.
//!
//! As the artwork file keeps a time-lapse, the frames can instead be in videos (`segment-1.mp4` on, the
//! frame each ends before in `segments.json`), the frames after them still pictures: a package's
//! videos come in as they are ([`TimeLapse::take_clips`]), and the frames a video made of them
//! holds can go ([`TimeLapse::drop_clip_frames`]). The app reads those frames from the videos
//! ([`TimeLapse::clips`]).
//!
//! The folder's layout is [`TIME_LAPSE_FORMAT`], kept in the note of its videos; every layout
//! before it is still read, and a note in an earlier one is written again in this one when the
//! time-lapse is opened. A note is read field by field: what can't be read takes its default, a
//! video it names that is gone leaves out only its own frames, and nothing is refused for being
//! of a later layout.

use std::fs;
use std::hash::{Hash, Hasher};
use std::io::{BufWriter, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;

use crate::layers::ThumbnailImage;
use crate::project::{PackageTimeLapse, file_error, write_atomically};

/// The replay's and the video's frames a second.
pub const TIME_LAPSE_FPS: u32 = 30;
/// A frame's longest side kept, pixels.
pub(crate) const FRAME_SIDE: u32 = 1280;
const JPEG_QUALITY: u8 = 85;
/// A package's videos are made again as one rather than be more than this (while every frame is
/// still a picture to make them of).
const MOST_SEGMENTS: usize = 32;

/// The layout of a time-lapse's folder, as `version` in its note of videos (`segments.json`):
/// - 1 (no `version`; every release to 0.0.6): the frames as `frame-000000.jpg` on, `settings.json`,
///   `off`, and the note's `resolution`, `quality`, `hevc`, `width`, `height`, `bitrate` and `ends`
///   (from 0.0.4 `clip_size` and `clip_hevc` too, and the pictures its videos hold gone).
/// - 2: as 1, the note saying `version`, the frames and videos written beside and then renamed in.
///
/// A change to what the folder holds or how its note reads takes the next number, with a reader
/// for this one kept (the format tests hold a recording in each).
pub const TIME_LAPSE_FORMAT: u32 = 2;

/// What changes the note of a folder's videos (planning, noting, dropping and taking them in)
/// happens one at a time in the process, so two objects on one folder (an artwork closing while it
/// opens again, a share meanwhile) can't each note a video of the same frames: the folder's lock,
/// one for every object on it, found by the artwork's folder (there before the time-lapse's is).
fn noting(artwork: &Path) -> Arc<Mutex<()>> {
    static FOLDERS: Mutex<Vec<(PathBuf, std::sync::Weak<Mutex<()>>)>> = Mutex::new(Vec::new());
    let key = fs::canonicalize(artwork).unwrap_or_else(|_| artwork.to_path_buf());
    let mut folders = FOLDERS.lock();
    folders.retain(|(_, lock)| lock.strong_count() > 0);
    if let Some(lock) = folders.iter().find(|(d, _)| *d == key).and_then(|(_, lock)| lock.upgrade()) {
        return lock;
    }
    let lock = Arc::new(Mutex::new(()));
    folders.push((key, Arc::downgrade(&lock)));
    lock
}

/// A video being made is named so, apart from the ones noted, until it is noted.
const MAKING: &str = "making-";
/// A frame being written is named so until it is whole.
const PART: &str = ".part";
/// The pictures being numbered again, written down (see [`TimeLapse::renumber`]).
const RENUMBERING: &str = "renumbering.json";

/// A time-lapse's video: 1080p, 2K or 4K (`resolution` 0 to 2), Low, Good, Studio quality or
/// Lossless (`quality` 0 to 3), and HEVC or H.264.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TimeLapseSettings {
    pub resolution: u32,
    pub quality: u32,
    pub hevc: bool,
}

impl Default for TimeLapseSettings {
    fn default() -> TimeLapseSettings {
        TimeLapseSettings { resolution: 0, quality: 1, hevc: false }
    }
}

/// The replay's and the video's frames a second.
#[uniffi::export]
pub fn time_lapse_fps() -> u32 {
    TIME_LAPSE_FPS
}

/// The longest side of a video made with `s`, pixels: 1920, 2732 or 3840.
#[uniffi::export]
pub fn time_lapse_side(s: TimeLapseSettings) -> u32 {
    match s.resolution {
        1 => 2732,
        2 => 3840,
        _ => 1920,
    }
}

/// The bits a pixel a second a video made with `s` takes: 2, 4, 8 or 16.
#[uniffi::export]
pub fn time_lapse_bits_per_pixel(s: TimeLapseSettings) -> u32 {
    match s.quality {
        0 => 2,
        2 => 8,
        3 => 16,
        _ => 4,
    }
}

/// How Canvas information names a time-lapse's settings: its quality, its resolution and its
/// codec (Good, 1080p, H.264).
#[uniffi::export]
pub fn time_lapse_setting_names(s: TimeLapseSettings) -> Vec<String> {
    let quality = ["Low", "Good", "Studio", "Lossless"].get(s.quality as usize).copied().unwrap_or("Good");
    let resolution = ["1080p", "2K", "4K"].get(s.resolution as usize).copied().unwrap_or("1080p");
    let codec = if s.hevc { "HEVC" } else { "H.264" };
    vec![quality.to_owned(), resolution.to_owned(), codec.to_owned()]
}

/// The frames a video `seconds` long takes out of `frames`, spread evenly; all of them when it
/// is none, or they wouldn't fill it.
#[uniffi::export]
pub fn time_lapse_picks(frames: u32, seconds: Option<u32>) -> Vec<u32> {
    let wanted = seconds.map(|s| s.saturating_mul(TIME_LAPSE_FPS));
    match wanted {
        Some(n) if frames > n && n > 0 => {
            let last = u64::from(frames - 1);
            let gaps = u64::from(n - 1).max(1);
            (0..u64::from(n)).filter_map(|i| u32::try_from(i * last / gaps).ok()).collect()
        }
        _ => (0..frames).collect(),
    }
}

#[derive(uniffi::Object)]
pub struct TimeLapse {
    dir: PathBuf,
    /// The folder's lock (see [`noting`]).
    noting: Arc<Mutex<()>>,
    state: Mutex<State>,
    /// A frame or its note couldn't be written (the disk full, say): [`Problem`], as a number.
    problem: std::sync::atomic::AtomicU8,
}

/// Where the time-lapse stands with writing: all written, or a write failed and not yet told, or
/// told (not again until a write works).
#[repr(u8)]
enum Problem {
    Fine = 0,
    Untold = 1,
    Told = 2,
}

/// What [`TimeLapse::take_problem`] says.
const PROBLEM_TEXT: &str =
    "The time-lapse couldn't be kept. The disk may be full: replay and export leave out what wasn't kept.";

struct State {
    /// Frames kept so far, and the last picture's hash (a picture the same as it isn't kept).
    count: u32,
    last: Option<u64>,
    /// A package's time-lapse being taken in.
    taking: Option<Taking>,
}

/// Taking a package's time-lapse in: its videos are kept (to go back into a package as they are)
/// while every frame of them has come in and there were no frames before; the frames each ends
/// before, the frames before the video being read, and the videos' size and bit rate.
struct Taking {
    whole: bool,
    ends: Vec<u32>,
    before: u32,
    width: u32,
    height: u32,
    bitrate: u64,
}

#[uniffi::export]
impl TimeLapse {
    /// The time-lapse of the artwork in folder `artwork`, its frames counted; a note of its videos
    /// in an earlier layout written again in this one, and videos whose note was lost taken back
    /// where no other frames are.
    #[uniffi::constructor]
    pub fn new(artwork: String) -> Arc<TimeLapse> {
        let dir = Path::new(&artwork).join("timelapse");
        let noting = noting(Path::new(&artwork));
        let lapse = TimeLapse {
            dir,
            noting,
            state: Mutex::new(State { count: 0, last: None, taking: None }),
            problem: std::sync::atomic::AtomicU8::new(Problem::Fine as u8),
        };
        let count = {
            let _noting = lapse.noting.lock();
            lapse.bring_up_to_date();
            lapse.counted()
        };
        lapse.state.lock().count = count;
        Arc::new(lapse)
    }

    /// The video's size, quality and codec, kept with the artwork.
    pub fn settings(&self) -> TimeLapseSettings {
        let text = fs::read_to_string(self.dir.join("settings.json")).unwrap_or_default();
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else { return TimeLapseSettings::default() };
        let whole = |key: &str, or: u32| {
            json.get(key).and_then(|v| v.as_u64()).and_then(|v| u32::try_from(v).ok()).unwrap_or(or)
        };
        TimeLapseSettings {
            resolution: whole("resolution", 0),
            quality: whole("quality", 1),
            hevc: json.get("hevc").and_then(|v| v.as_bool()).unwrap_or(false),
        }
    }

    pub fn set_settings(&self, s: TimeLapseSettings) {
        let json = serde_json::json!({ "resolution": s.resolution, "quality": s.quality, "hevc": s.hevc });
        let written = fs::create_dir_all(&self.dir).map_err(|e| e.to_string()).and_then(|()| {
            write_atomically(&self.dir.join("settings.json"), |p| {
                fs::write(p, json.to_string()).map_err(|e| file_error("saving", e))
            })
            .map_err(|e| e.to_string())
        });
        if let Err(e) = written {
            log::warn!("keeping the time-lapse's settings: {e}");
        }
    }

    /// Whether changes are recorded (on unless turned off for this artwork).
    pub fn recording(&self) -> bool {
        !self.dir.join("off").exists()
    }

    pub fn set_recording(&self, on: bool) {
        let off = self.dir.join("off");
        let done = if on {
            match fs::remove_file(&off) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            }
        } else {
            fs::create_dir_all(&self.dir).and_then(|()| fs::write(&off, b""))
        };
        if let Err(e) = done {
            log::warn!("switching the time-lapse: {e}");
        }
    }

    /// Frames recorded so far.
    pub fn count(&self) -> u32 {
        self.state.lock().count
    }

    /// Deletes what has been recorded (its pictures and videos), as asked when recording is
    /// turned off; the settings, and whether it records, stay.
    pub fn purge(&self) {
        let _noting = self.noting.lock();
        let mut st = self.state.lock();
        for f in frame_files(&self.dir) {
            let _ = fs::remove_file(f);
        }
        let _ = fs::remove_file(self.dir.join("segments.json"));
        let _ = fs::remove_file(self.dir.join(RENUMBERING));
        if let Ok(entries) = fs::read_dir(&self.dir) {
            for path in entries.filter_map(|e| e.ok()).map(|e| e.path()) {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                let video = name.starts_with("segment-") || name.starts_with(SET_ASIDE) || name.starts_with(MAKING);
                if video && name.ends_with(".mp4") {
                    let _ = fs::remove_file(&path);
                }
            }
        }
        st.count = 0;
        st.last = None;
        st.taking = None;
    }

    /// What the recorded frames take on disk (pictures and videos), bytes.
    pub fn bytes(&self) -> u64 {
        let videos = self.clips().into_iter().map(|c| PathBuf::from(c.path));
        frame_files(&self.dir).into_iter().chain(videos).filter_map(|f| fs::metadata(f).ok()).map(|m| m.len()).sum()
    }

    /// Keeps a picture of the canvas (premultiplied RGBA, `width` × `height`, as
    /// [`crate::CanvasSession::time_lapse_frame`] makes it) as the next frame, on white (as the
    /// canvas shows a hidden background) and at most 1280 on its long side; unless it isn't
    /// recording, or the picture is the same as the last. True when kept.
    pub fn record(&self, rgba: Vec<u8>, width: u32, height: u32) -> bool {
        let len = (width as usize).saturating_mul(height as usize).saturating_mul(4);
        if !self.recording() || width == 0 || height == 0 || rgba.len() < len {
            return false;
        }
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        rgba.hash(&mut hasher);
        let hash = hasher.finish();
        let mut st = self.state.lock();
        if st.last == Some(hash) {
            return false;
        }
        st.last = Some(hash);
        // premultiplied over white
        let rgb: Vec<u8> = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|&[r, g, b, a]| {
                let under = 255 - u16::from(a);
                [r, g, b].map(|c| (u16::from(c) + under).min(255) as u8)
            })
            .collect();
        let Some(picture) = image::RgbImage::from_raw(width, height, rgb) else { return false };
        self.keep(&mut st, picture)
    }

    /// Takes a package's time-lapse `t` in: its settings and whether it records; then the app
    /// gives its videos' frames ([`Self::keep_frame`], [`Self::take_video`]) and ends with
    /// [`Self::take_end`].
    pub fn take_begin(&self, t: PackageTimeLapse) {
        self.set_settings(TimeLapseSettings {
            resolution: u32::from(t.resolution),
            quality: u32::from(t.quality),
            hevc: t.hevc,
        });
        self.set_recording(t.recording);
        let _noting = self.noting.lock();
        self.drop_segments();
        let mut st = self.state.lock();
        st.taking = Some(Taking {
            whole: st.count == 0 && t.width > 0 && t.height > 0,
            ends: Vec::new(),
            before: st.count,
            width: t.width,
            height: t.height,
            bitrate: t.bitrate,
        });
    }

    /// A frame of a package's video (straight RGBA, opaque) kept as the next frame. True when
    /// kept.
    pub fn keep_frame(&self, rgba: Vec<u8>, width: u32, height: u32) -> bool {
        let rgb: Vec<u8> = rgba.as_chunks::<4>().0.iter().flat_map(|&[r, g, b, _]| [r, g, b]).collect();
        let Some(picture) = image::RgbImage::from_raw(width, height, rgb) else { return false };
        let mut st = self.state.lock();
        self.keep(&mut st, picture)
    }

    /// The package's video at `path` was read, `frames` frames in it: it is kept as it is while
    /// every frame of every video so far came in.
    pub fn take_video(&self, path: String, frames: u32) {
        let _noting = self.noting.lock();
        let mut st = self.state.lock();
        let count = st.count;
        let Some(t) = st.taking.as_mut() else { return };
        t.whole = t.whole && frames > 0 && count - t.before == frames;
        if t.whole {
            let to = self.segment_path(t.ends.len());
            t.whole = fs::create_dir_all(&self.dir).and_then(|()| fs::copy(&path, &to)).is_ok();
        }
        t.ends.push(count);
        t.before = count;
    }

    /// The package's time-lapse is in: its videos noted, if they were all kept.
    pub fn take_end(&self) {
        let _noting = self.noting.lock();
        let Some(t) = self.state.lock().taking.take() else { return };
        if t.whole && !t.ends.is_empty() && t.ends.len() < MOST_SEGMENTS {
            let s = self.settings();
            // as its videos are, where their header says
            let made = mp4_frames(Path::new(&self.segment_path(0)));
            self.write_segments(&Segments {
                version: TIME_LAPSE_FORMAT,
                resolution: s.resolution,
                quality: s.quality,
                hevc: s.hevc,
                width: t.width,
                height: t.height,
                bitrate: t.bitrate,
                ends: t.ends,
                clip_size: made.map(|m| (m.width, m.height)),
                clip_hevc: made.map(|m| m.hevc),
            });
        } else {
            // what this take copied is no one's only copy, so isn't set aside
            for i in 0..t.ends.len() {
                let _ = fs::remove_file(self.segment_path(i));
            }
            self.drop_segments();
        }
    }
}

impl TimeLapse {
    /// Keeps `picture` as the next frame, at most 1280 on its long side, as JPEG.
    fn keep(&self, st: &mut State, picture: image::RgbImage) -> bool {
        let (width, height) = picture.dimensions();
        let k = (FRAME_SIDE as f32 / width.max(height) as f32).min(1.0);
        let picture = if k < 1.0 {
            let (w, h) = (((width as f32 * k).round() as u32).max(1), ((height as f32 * k).round() as u32).max(1));
            image::imageops::resize(&picture, w, h, image::imageops::FilterType::Triangle)
        } else {
            picture
        };
        let path = self.frame_path(st.count);
        // whole or not at all: the app stopped mid-write (an update, the system) leaves no
        // broken frame
        let part = PathBuf::from(format!("{}{PART}", path.to_string_lossy()));
        let written = fs::create_dir_all(&self.dir).map_err(|e| e.to_string()).and_then(|()| {
            let file = fs::File::create(&part).map_err(|e| e.to_string())?;
            let mut out = BufWriter::new(file);
            let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY);
            picture.write_with_encoder(encoder).map_err(|e| e.to_string())?;
            drop(out.into_inner().map_err(|e| e.to_string())?);
            fs::rename(&part, &path).map_err(|e| e.to_string())
        });
        if written.is_err() {
            let _ = fs::remove_file(&part);
        }
        match written {
            Ok(()) => {
                st.count += 1;
                self.problem.store(Problem::Fine as u8, std::sync::atomic::Ordering::Relaxed);
                true
            }
            Err(e) => {
                log::warn!("keeping a time-lapse frame: {e}");
                self.failed();
                false
            }
        }
    }

    /// A write failed: to be told, unless it was and nothing has been written since.
    fn failed(&self) {
        let _ = self.problem.compare_exchange(
            Problem::Fine as u8,
            Problem::Untold as u8,
            std::sync::atomic::Ordering::Relaxed,
            std::sync::atomic::Ordering::Relaxed,
        );
    }
}

#[uniffi::export]
impl TimeLapse {
    /// What to tell, once, when the time-lapse couldn't be written (the disk full, say); none
    /// otherwise, and none again until a write has worked and another failed.
    pub fn take_problem(&self) -> Option<String> {
        self.problem
            .compare_exchange(
                Problem::Untold as u8,
                Problem::Told as u8,
                std::sync::atomic::Ordering::Relaxed,
                std::sync::atomic::Ordering::Relaxed,
            )
            .is_ok()
            .then(|| PROBLEM_TEXT.to_owned())
    }

    /// Frame `i`, straight RGBA (opaque); none if there is no such frame.
    pub fn frame(&self, i: u32) -> Option<ThumbnailImage> {
        let picture = image::open(self.frame_path(i)).ok()?.to_rgba8();
        Some(ThumbnailImage { width: picture.width(), height: picture.height(), rgba: picture.into_raw() })
    }

    /// Frame `i`'s file (a JPEG), for an app that reads it with its own decoder; none if there is
    /// no such frame.
    pub fn frame_file(&self, i: u32) -> Option<String> {
        let path = self.frame_path(i);
        path.exists().then(|| path.to_string_lossy().into_owned())
    }

    /// Whether frame `i` is kept: as a picture, or in a video (which a device may not decode). An
    /// app that can't read a frame says by this whether it is gone or only can't be decoded there.
    pub fn frame_kept(&self, i: u32) -> bool {
        self.frame_path(i).exists() || self.clips().iter().any(|c| i >= c.first && i - c.first < c.frames)
    }

    /// Every frame is in a video noted already, so a full-length video is those joined.
    pub fn all_in_videos(&self) -> bool {
        let end = self.note().and_then(|k| k.ends.last().copied()).unwrap_or(0);
        let holes = self.clips().iter().map(|c| c.frames).sum::<u32>() != end;
        !holes && end >= self.count()
    }
}

/// A video a package needs made of the frames: into `path`, of frames `frames`, fitted on white
/// into `width` x `height` (0 x 0: into the first frame's size, within the settings' side);
/// `append` when it adds to the videos made before (it must then come out that size, and as HEVC
/// or H.264 as `hevc` says, as they are). `path` is its own, beside the videos noted: it goes in
/// with them only if no other was noted since it was planned, when `noted` were.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SegmentPlan {
    pub path: String,
    pub frames: Vec<u32>,
    pub width: u32,
    pub height: u32,
    pub append: bool,
    pub hevc: bool,
    pub noted: u32,
}

/// One of a time-lapse's videos: its file, the frame it starts at, and how many it holds.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TimeLapseClip {
    pub path: String,
    pub first: u32,
    pub frames: u32,
}

/// The videos made of the frames for packages: the layout they are noted in, the settings they
/// were made with, their size and bit rate, and the frame each ends before (`ends`, rising), their
/// files numbered from 1.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
struct Segments {
    version: u32,
    resolution: u32,
    quality: u32,
    hevc: bool,
    width: u32,
    height: u32,
    bitrate: u64,
    ends: Vec<u32>,
    /// The videos' own size and codec where they aren't what the package notes (a package's may
    /// say HEVC at one size of videos made as H.264 at another): what one added to them is made
    /// as.
    clip_size: Option<(u32, u32)>,
    clip_hevc: Option<bool>,
}

impl Segments {
    /// The size and codec a video added to these is made as (a size of 0: not known).
    fn made_as(&self) -> (u32, u32, bool) {
        let (w, h) = self.clip_size.unwrap_or((self.width, self.height));
        (w, h, self.clip_hevc.unwrap_or(self.hevc))
    }

    /// A note as any layout wrote it, read field by field: one that can't be read takes its
    /// default, and `ends` is its run of rising whole numbers from the first (the frames after one
    /// that can't be read can't be placed). None if it isn't a note at all.
    fn read(text: &str) -> Option<Segments> {
        use serde_json::Value;
        let json: Value = serde_json::from_str(text).ok()?;
        let note = json.as_object()?;
        let whole = |v: &Value| v.as_u64().and_then(|v| u32::try_from(v).ok());
        let field = |key: &str, or: u32| note.get(key).and_then(whole).unwrap_or(or);
        let mut ends: Vec<u32> = Vec::new();
        for end in note.get("ends")?.as_array()? {
            match whole(end) {
                Some(e) if e > ends.last().copied().unwrap_or(0) => ends.push(e),
                _ => break,
            }
        }
        let clip_size = note.get("clip_size").and_then(Value::as_array).and_then(|size| match size.as_slice() {
            [w, h] => Some((whole(w)?, whole(h)?)),
            _ => None,
        });
        let version = field("version", 1);
        if version > TIME_LAPSE_FORMAT {
            log::info!("a time-lapse noted in layout {version}, later than this version's: read as far as it can be");
        }
        Some(Segments {
            version,
            resolution: field("resolution", 0),
            quality: field("quality", 1),
            hevc: note.get("hevc").and_then(Value::as_bool).unwrap_or(false),
            width: field("width", 0),
            height: field("height", 0),
            bitrate: note.get("bitrate").and_then(Value::as_u64).unwrap_or(0),
            ends,
            clip_size,
            clip_hevc: note.get("clip_hevc").and_then(Value::as_bool),
        })
    }
}

/// What an MP4's header says of its video: its frames, picture size and whether it is HEVC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Mp4 {
    pub frames: u32,
    pub width: u32,
    pub height: u32,
    pub hevc: bool,
}

/// What a video is named after, with no note saying what it holds, so kept rather than lost.
const SET_ASIDE: &str = "set-aside-";

#[uniffi::export]
impl TimeLapse {
    /// The video a package still needs made so every frame is in one (the ones made before kept,
    /// so a share makes only what is new); none when they all are. The ones made before go when
    /// the settings changed, they are broken, or there would be too many, while their frames are
    /// still pictures to make them again of.
    pub fn segment_plan(&self) -> Option<SegmentPlan> {
        let _noting = self.noting.lock();
        self.plan()
    }

    /// `plan`'s video was made `width` x `height` (or `made` false: it couldn't be). It goes in
    /// with the videos noted when it holds every frame planned, no other was noted meanwhile and,
    /// added to those, it is their size; else it goes. Another video was noted meanwhile: what is
    /// still to make, planned again. A video added to the ones before that failed means all are
    /// made again as one: that plan; unless the ones before hold the only copy of their frames
    /// (they stay, and the frames since stay out of the package this time).
    pub fn segment_made(&self, plan: SegmentPlan, width: u32, height: u32, made: bool) -> Option<SegmentPlan> {
        let _noting = self.noting.lock();
        let at = Path::new(&plan.path);
        if made {
            let kept = self.kept_segments();
            let noted = kept.as_ref().map_or(0, |k| k.ends.len());
            let done = kept.as_ref().and_then(|k| k.ends.last().copied()).unwrap_or(0);
            let first = plan.frames.first().copied().unwrap_or(0);
            let same = usize::try_from(plan.noted).ok() == Some(noted) && (!plan.append || first == done);
            if !same || (!plan.append && noted > 0) {
                // (the artwork closed twice in a row, a share meanwhile)
                log::info!("a time-lapse video was noted while another of the same frames was made: this one goes");
                let _ = fs::remove_file(at);
                return self.plan();
            }
            // (a video cut short, the disk full, has no header to read)
            let holds = mp4_frames(at).is_some_and(|m| usize::try_from(m.frames).ok() == Some(plan.frames.len()));
            let sized = !plan.append
                || kept.as_ref().is_some_and(|k| {
                    let (w, h, _) = k.made_as();
                    w == 0 || h == 0 || (w, h) == (width, height)
                });
            let end = plan.frames.last().map_or(0, |f| f + 1);
            if holds && sized && end > done {
                let to = self.segment_path(noted);
                self.set_aside(Path::new(&to));
                if fs::rename(at, &to).is_ok() {
                    let segments = match kept {
                        Some(mut k) if plan.append => {
                            k.ends.push(end);
                            if k.made_as().0 == 0 || k.made_as().1 == 0 {
                                k.clip_size = Some((width, height));
                            }
                            k.version = TIME_LAPSE_FORMAT;
                            k
                        }
                        _ => {
                            let s = self.settings();
                            let bits = time_lapse_bits_per_pixel(s);
                            Segments {
                                version: TIME_LAPSE_FORMAT,
                                resolution: s.resolution,
                                quality: s.quality,
                                hevc: s.hevc,
                                width,
                                height,
                                bitrate: (u64::from(width) * u64::from(height) * u64::from(bits)).max(1_000_000),
                                ends: vec![end],
                                clip_size: None,
                                clip_hevc: None,
                            }
                        }
                    };
                    self.write_segments(&segments);
                    return None;
                }
                log::warn!("a time-lapse video couldn't be put with the others");
            } else if !holds {
                log::warn!(
                    "a time-lapse video holds other than the {} frames it was made of: not noted, their pictures stay",
                    plan.frames.len()
                );
            } else {
                log::warn!("a time-lapse video came out another size than those before it: not noted");
            }
        }
        // (not all of what makes them clears up after a failure)
        let _ = fs::remove_file(at);
        if self.note().is_some_and(|k| !self.remakeable(&k)) {
            return None;
        }
        self.drop_segments();
        plan.append.then(|| SegmentPlan {
            path: self.making_path(0),
            frames: (0..self.count()).collect(),
            width: 0,
            height: 0,
            append: false,
            hevc: self.settings().hevc,
            noted: 0,
        })
    }

    /// The videos the frames are in, in order, each where its frames are (the frames after the
    /// last are pictures; a video noted that is gone leaves out only its own frames).
    pub fn clips(&self) -> Vec<TimeLapseClip> {
        let Some(k) = self.note() else { return Vec::new() };
        let mut first = 0;
        let mut clips = Vec::with_capacity(k.ends.len());
        for (i, &end) in k.ends.iter().enumerate() {
            let path = self.segment_path(i);
            if Path::new(&path).exists() {
                clips.push(TimeLapseClip { path, first, frames: end.saturating_sub(first) });
            }
            first = end;
        }
        clips
    }

    /// Takes a package's time-lapse `t` in as the package keeps it: its videos as they are (moved
    /// here), their frames counted from their headers, never decoded. False, nothing changed,
    /// when there are frames here already, it has no videos, or a video's frames can't be
    /// counted: then it is taken in frame by frame ([`Self::take_begin`]).
    pub fn take_clips(&self, t: PackageTimeLapse) -> bool {
        if self.count() > 0 || t.segments.is_empty() {
            return false;
        }
        let mut counted = Vec::with_capacity(t.segments.len());
        for s in &t.segments {
            match mp4_frames(Path::new(s)) {
                Some(c) if c.frames > 0 => counted.push(c),
                _ => return false,
            }
        }
        let _noting = self.noting.lock();
        if fs::create_dir_all(&self.dir).is_err() {
            return false;
        }
        let mut ends = Vec::with_capacity(counted.len());
        let mut end = 0u32;
        for (i, (s, Mp4 { frames, .. })) in t.segments.iter().zip(&counted).enumerate() {
            let to = self.segment_path(i);
            self.set_aside(Path::new(&to));
            if fs::rename(s, &to).or_else(|_| fs::copy(s, &to).map(|_| ())).is_err() {
                for j in 0..i {
                    let _ = fs::remove_file(self.segment_path(j));
                }
                return false;
            }
            end = end.saturating_add(*frames);
            ends.push(end);
        }
        // as the package notes it, and as its videos are
        let made = counted.first().copied();
        let (width, height) = match made {
            _ if t.width > 0 && t.height > 0 => (t.width, t.height),
            Some(m) => (m.width, m.height),
            None => (0, 0),
        };
        self.set_settings(TimeLapseSettings {
            resolution: u32::from(t.resolution),
            quality: u32::from(t.quality),
            hevc: t.hevc,
        });
        self.set_recording(t.recording);
        self.write_segments(&Segments {
            version: TIME_LAPSE_FORMAT,
            resolution: u32::from(t.resolution),
            quality: u32::from(t.quality),
            hevc: t.hevc,
            width,
            height,
            bitrate: t.bitrate,
            ends,
            clip_size: made.map(|m| (m.width, m.height)),
            clip_hevc: made.map(|m| m.hevc),
        });
        self.state.lock().count = end;
        true
    }

    /// The videos whose frames are still pictures too: what an app reads back before those
    /// pictures go ([`Self::drop_clip_frames`]), a header not saying that a device can read it.
    pub fn clips_with_pictures(&self) -> Vec<TimeLapseClip> {
        self.clips().into_iter().filter(|c| self.frame_path(c.first).exists()).collect()
    }

    /// The frames kept as pictures that its videos hold too, gone (as the artwork file keeps a
    /// time-lapse: only as video); of each video only once its frames, counted from its header,
    /// are as many as it is noted to hold. How many went.
    pub fn drop_clip_frames(&self) -> u32 {
        let _noting = self.noting.lock();
        let holding: Vec<TimeLapseClip> = self
            .clips()
            .into_iter()
            .filter(|c| mp4_frames(Path::new(&c.path)).is_some_and(|m| m.frames == c.frames))
            .collect();
        let mut gone = 0;
        for f in frame_files(&self.dir) {
            let held = frame_index(&f).is_some_and(|i| holding.iter().any(|c| i >= c.first && i - c.first < c.frames));
            if held && fs::remove_file(&f).is_ok() {
                gone += 1;
            }
        }
        gone
    }

    /// The time-lapse as a package keeps it: its settings, and its videos once made.
    pub fn for_package(&self) -> PackageTimeLapse {
        let s = self.settings();
        let kept = self.kept_segments();
        let segments = if kept.is_some() { self.clips().into_iter().map(|c| c.path).collect() } else { Vec::new() };
        PackageTimeLapse {
            recording: self.recording(),
            quality: u8::try_from(s.quality).unwrap_or(1),
            resolution: u8::try_from(s.resolution).unwrap_or(0),
            hevc: s.hevc,
            segments,
            width: kept.as_ref().map_or(0, |k| k.width),
            height: kept.as_ref().map_or(0, |k| k.height),
            fps: TIME_LAPSE_FPS,
            bitrate: kept.as_ref().map_or(0, |k| k.bitrate),
        }
    }
}

impl TimeLapse {
    fn frame_path(&self, i: u32) -> PathBuf {
        self.dir.join(format!("frame-{i:06}.jpg"))
    }

    fn segment_path(&self, i: usize) -> String {
        self.dir.join(format!("segment-{}.mp4", i + 1)).to_string_lossy().into_owned()
    }

    /// Where a video planned as the `i`th is made: a name of its own, out of the way of the ones
    /// noted and of another being made.
    fn making_path(&self, i: usize) -> String {
        static MADE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = MADE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let name = format!("{MAKING}{}-{n}-segment-{}.mp4", std::process::id(), i + 1);
        self.dir.join(name).to_string_lossy().into_owned()
    }

    /// The plan as [`Self::segment_plan`] gives it, the noting held.
    fn plan(&self) -> Option<SegmentPlan> {
        let frames = self.count();
        let kept = self.kept_segments();
        if kept.is_none() {
            self.drop_segments();
        }
        let done = kept.as_ref().and_then(|k| k.ends.last().copied()).unwrap_or(0);
        if done >= frames {
            return None;
        }
        Some(match kept {
            Some(k) => {
                let (width, height, hevc) = k.made_as();
                SegmentPlan {
                    path: self.making_path(k.ends.len()),
                    frames: (done..frames).collect(),
                    width,
                    height,
                    append: true,
                    hevc,
                    noted: u32::try_from(k.ends.len()).unwrap_or(u32::MAX),
                }
            }
            None => SegmentPlan {
                path: self.making_path(0),
                frames: (0..frames).collect(),
                width: 0,
                height: 0,
                append: false,
                hevc: self.settings().hevc,
                noted: 0,
            },
        })
    }

    /// The videos made before, when they still hold: as [`Self::note`], every one there, and made
    /// with the settings now and not too many; or, whatever the settings, holding the only copy
    /// of their frames.
    fn kept_segments(&self) -> Option<Segments> {
        let k = self.note()?;
        let s = self.settings();
        let whole = self.clips().len() == k.ends.len();
        let fresh = (k.resolution, k.quality, k.hevc) == (s.resolution, s.quality, s.hevc)
            && k.ends.len() < MOST_SEGMENTS
            && whole;
        (fresh || !self.remakeable(&k)).then_some(k)
    }

    /// The note of the videos, read as any layout wrote it ([`Segments::read`]), those named at its
    /// end that are gone left out (a video noted that was never made); none without one, or with
    /// none of its videos there. A size it doesn't say is its first video's.
    fn note(&self) -> Option<Segments> {
        let text = fs::read_to_string(self.dir.join("segments.json")).ok()?;
        let mut k = Segments::read(&text)?;
        while k.ends.len().checked_sub(1).is_some_and(|last| !Path::new(&self.segment_path(last)).exists()) {
            k.ends.pop();
        }
        if k.ends.is_empty() {
            return None;
        }
        if k.width == 0 || k.height == 0 {
            let first = (0..k.ends.len()).map(|i| self.segment_path(i)).find(|p| Path::new(p).exists());
            if let Some(m) = first.and_then(|p| mp4_frames(Path::new(&p))) {
                (k.width, k.height) = (m.width, m.height);
            }
        }
        Some(k)
    }

    /// Every frame in videos `k` is still a picture too, to make them again of.
    fn remakeable(&self, k: &Segments) -> bool {
        let end = k.ends.last().copied().unwrap_or(0);
        let pictures = frame_files(&self.dir).iter().filter(|f| frame_index(f).is_some_and(|i| i < end)).count();
        pictures == end as usize
    }

    /// The frames recorded: up to the last a video or a picture holds (a frame missing between
    /// them is one a reader leaves out, and the next is recorded after them all).
    fn counted(&self) -> u32 {
        let videos = self.note().and_then(|k| k.ends.last().copied()).unwrap_or(0);
        let pictures =
            frame_files(&self.dir).iter().filter_map(|f| frame_index(f)).max().map_or(0, |i| i.saturating_add(1));
        videos.max(pictures)
    }

    /// The folder as this version keeps it, the noting held: what a stop mid-write left (a frame
    /// half written, a video half made) gone; a note in an earlier layout, or naming videos at its
    /// end that were never made, written again (whole, beside, then renamed in); and with no note,
    /// videos whose frames are nowhere else taken back ([`Self::take_back`]).
    fn bring_up_to_date(&self) {
        let Ok(entries) = fs::read_dir(&self.dir) else { return };
        let stale = |p: &Path, after: std::time::Duration| {
            fs::metadata(p)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age > after)
        };
        for path in entries.filter_map(|e| e.ok()).map(|e| e.path()) {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let half_frame = name.starts_with("frame-") && name.ends_with(PART);
            // (another object here may be making one now: only those a day old)
            let half_video = name.starts_with(MAKING) && stale(&path, std::time::Duration::from_secs(24 * 3600));
            if (half_frame && stale(&path, std::time::Duration::from_secs(60))) || half_video {
                let _ = fs::remove_file(&path);
            }
        }
        // a renumbering cut short, finished before anything is read
        if !self.finish_renumbering() {
            return;
        }
        let text = fs::read_to_string(self.dir.join("segments.json")).ok();
        match (text.as_deref().and_then(Segments::read), self.note()) {
            (Some(read), Some(note)) => {
                // (a later layout's note is left as it is: what it holds besides isn't known here)
                let behind = read.version < TIME_LAPSE_FORMAT || read.ends.len() != note.ends.len();
                if read.version <= TIME_LAPSE_FORMAT && behind {
                    log::info!(
                        "a time-lapse's note in layout {} ({} videos, {} there) written again in layout {TIME_LAPSE_FORMAT}",
                        read.version,
                        read.ends.len(),
                        note.ends.len()
                    );
                    self.write_segments(&Segments { version: TIME_LAPSE_FORMAT, ..note });
                }
                self.put_back_before();
            }
            _ => self.take_back(),
        }
    }

    /// Videos set aside together (by an earlier version, once it couldn't read their note) that
    /// were all made before anything the time-lapse holds now: the frames before it, which that
    /// version went on without. Put back in front of it, the newest set first, its videos and
    /// pictures numbered on after them (moves written down first, as [`Self::renumber`]'s).
    fn put_back_before(&self) {
        for group in (0..64u32).rev() {
            let run = self.aside_run(group);
            let Some(made) = run.iter().filter_map(|(p, _)| written(p)).max() else { continue };
            let Some(note) = self.note() else { return };
            let now: Vec<PathBuf> = (0..note.ends.len())
                .map(|i| PathBuf::from(self.segment_path(i)))
                .chain(frame_files(&self.dir).into_iter().filter(|f| frame_index(f).is_some()))
                .collect();
            if now.iter().filter_map(|p| written(p)).min().is_none_or(|first| made > first) {
                continue;
            }
            // (what that version set aside is two videos or more; one alone is what a stop
            // mid-close left, and its frames are in the first noted; and the frames noted since are
            // new ones: a video made again of the same frames would hold all of them)
            let total = run.iter().fold(0u32, |n, (_, v)| n.saturating_add(v.frames));
            if run.len() < 2 || note.ends.first().is_none_or(|&first| first >= total) {
                continue;
            }
            let m = run.len();
            let pictures: Vec<u32> = frame_files(&self.dir).iter().filter_map(|f| frame_index(f)).collect();
            let moves: Vec<(u32, u32)> = pictures.iter().map(|&i| (i, i.saturating_add(total))).collect();
            let name = |p: &Path| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            // the videos noted now one place on for each put back (the last first), then those
            let mut videos: Vec<(String, String)> = (0..note.ends.len())
                .rev()
                .map(|j| (name(Path::new(&self.segment_path(j))), name(Path::new(&self.segment_path(j + m)))))
                .collect();
            videos.extend(run.iter().enumerate().map(|(i, (p, _))| (name(p), name(Path::new(&self.segment_path(i))))));
            let mut ends = Vec::with_capacity(m + note.ends.len());
            let mut end = 0u32;
            for (_, v) in &run {
                end = end.saturating_add(v.frames);
                ends.push(end);
            }
            ends.extend(note.ends.iter().map(|e| e.saturating_add(total)));
            let after = Segments { version: TIME_LAPSE_FORMAT, ends, ..note };
            let plan = serde_json::json!({ "moves": moves, "placing": false, "videos": videos, "note": after });
            log::info!("a time-lapse's {m} videos set aside, {total} frames made before it, put back in front of it");
            let kept = write_atomically(&self.dir.join(RENUMBERING), |p| {
                fs::write(p, plan.to_string()).map_err(|e| file_error("saving", e))
            });
            if kept.is_err() || !self.finish_renumbering() {
                return;
            }
        }
    }

    /// The videos set aside together as group `group` (`set-aside-0-segment-1.mp4` on), in order,
    /// while each has a header to count its frames by.
    fn aside_run(&self, group: u32) -> Vec<(PathBuf, Mp4)> {
        (0..256)
            .map_while(|i| {
                let path = self.dir.join(format!("{SET_ASIDE}{group}-segment-{}.mp4", i + 1));
                mp4_frames(&path).filter(|m| m.frames > 0).map(|m| (path, m))
            })
            .collect()
    }

    /// With no note of them, the videos (`segment-1.mp4` on, else those set aside together) noted
    /// again as the frames from the first, when the first frame is no picture: they are what a
    /// lost note left of those frames. Where they hold more frames than come before the first
    /// picture, the pictures were numbered afresh after the note was lost (an earlier version
    /// counted only them): they are numbered again after the videos, in the order they were
    /// recorded ([`Self::renumber`]).
    fn take_back(&self) {
        let first_picture = frame_files(&self.dir).iter().filter_map(|f| frame_index(f)).min().unwrap_or(u32::MAX);
        if first_picture == 0 {
            return;
        }
        let run = |name: &dyn Fn(usize) -> PathBuf| -> Vec<(PathBuf, Mp4)> {
            (0..256)
                .map_while(|i| {
                    let path = name(i);
                    mp4_frames(&path).filter(|m| m.frames > 0).map(|m| (path, m))
                })
                .collect()
        };
        let mut found = run(&|i| PathBuf::from(self.segment_path(i)));
        let mut aside = false;
        if found.is_empty()
            && let Some(videos) = (0..64u32).map(|group| self.aside_run(group)).find(|run| !run.is_empty())
        {
            found = videos;
            aside = true;
        }
        let Some(&(_, m)) = found.first() else { return };
        let total = found.iter().fold(0u32, |n, (_, v)| n.saturating_add(v.frames));
        if total > first_picture && first_picture != u32::MAX && !self.renumber(&found) {
            log::warn!(
                "a time-lapse's videos with no note of them can't be put back before its pictures: left as they are"
            );
            return;
        }
        let mut ends = Vec::with_capacity(found.len());
        let mut end = 0u32;
        for (i, (path, video)) in found.iter().enumerate() {
            let to = self.segment_path(i);
            if aside {
                self.set_aside(Path::new(&to));
                if fs::rename(path, &to).is_err() {
                    break;
                }
            }
            end = end.saturating_add(video.frames);
            ends.push(end);
        }
        if ends.is_empty() {
            return;
        }
        let s = self.settings();
        log::info!("a time-lapse's {} videos with no note of them noted again: frames 0 to {end}", ends.len());
        self.write_segments(&Segments {
            version: TIME_LAPSE_FORMAT,
            resolution: s.resolution,
            quality: s.quality,
            hevc: s.hevc,
            width: m.width,
            height: m.height,
            bitrate: (u64::from(m.width) * u64::from(m.height) * u64::from(time_lapse_bits_per_pixel(s)))
                .max(1_000_000),
            ends,
            clip_size: None,
            clip_hevc: Some(m.hevc),
        });
    }

    /// The pictures numbered again after `videos` (the frames from the first on), in the order they
    /// were recorded (when each was written, then its number); but for one written before the video
    /// whose frames take in its number, which is that frame of it, kept where it is. The moves are
    /// written down first (whole, beside, then renamed in), so a renumbering cut short is finished
    /// the next time ([`Self::finish_renumbering`]).
    fn renumber(&self, videos: &[(PathBuf, Mp4)]) -> bool {
        let written = |p: &Path| fs::metadata(p).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
        let mut spans = Vec::with_capacity(videos.len());
        let mut from = 0u32;
        for (path, video) in videos {
            let end = from.saturating_add(video.frames);
            spans.push((from, end, written(path)));
            from = end;
        }
        let mut pictures: Vec<(std::time::SystemTime, u32)> = frame_files(&self.dir)
            .iter()
            .filter_map(|f| {
                let (when, i) = (written(f), frame_index(f)?);
                let in_a_video = spans.iter().any(|&(first, end, made)| i >= first && i < end && when <= made);
                (!in_a_video).then_some((when, i))
            })
            .collect();
        pictures.sort_unstable();
        let moves: Vec<(u32, u32)> = pictures.iter().zip(from..).map(|(&(_, was), now)| (was, now)).collect();
        let plan = serde_json::json!({ "moves": moves, "placing": false });
        let written = write_atomically(&self.dir.join(RENUMBERING), |p| {
            fs::write(p, plan.to_string()).map_err(|e| file_error("saving", e))
        });
        if let Err(e) = written {
            log::warn!("numbering a time-lapse's pictures again: {e}");
            return false;
        }
        log::info!("a time-lapse's {} pictures numbered again from {from}, after its videos", moves.len());
        self.finish_renumbering()
    }

    /// A renumbering written down ([`Self::renumber`]) carried out, from where it was cut short:
    /// each picture first out of the way under its new number, then (that done, and written
    /// down) each into its place. True when there is none left to do.
    fn finish_renumbering(&self) -> bool {
        let note = self.dir.join(RENUMBERING);
        let Ok(text) = fs::read_to_string(&note) else { return true };
        let plan: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
        let moves: Vec<(u32, u32)> =
            plan.get("moves").and_then(|m| serde_json::from_value(m.clone()).ok()).unwrap_or_default();
        let out_of_the_way = |to: u32| self.dir.join(format!("frame-{to:06}.jpg.renumbered"));
        if !plan.get("placing").and_then(serde_json::Value::as_bool).unwrap_or(false) {
            for &(was, to) in &moves {
                let from = self.frame_path(was);
                if from.exists() && !out_of_the_way(to).exists() && fs::rename(&from, out_of_the_way(to)).is_err() {
                    return false;
                }
            }
            let mut placing = plan.clone();
            if let Some(p) = placing.as_object_mut() {
                p.insert("placing".to_owned(), serde_json::Value::Bool(true));
            }
            let written =
                write_atomically(&note, |p| fs::write(p, placing.to_string()).map_err(|e| file_error("saving", e)));
            if written.is_err() {
                return false;
            }
        }
        for &(_, to) in &moves {
            let aside = out_of_the_way(to);
            if aside.exists() && fs::rename(&aside, self.frame_path(to)).is_err() {
                return false;
            }
        }
        // videos moved (each once: where it goes is taken when it has), then their note
        let videos: Vec<(String, String)> =
            plan.get("videos").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
        for (from, to) in &videos {
            let (from, to) = (self.dir.join(from), self.dir.join(to));
            if from.exists() && !to.exists() && fs::rename(&from, &to).is_err() {
                return false;
            }
        }
        if let Some(after) = plan.get("note") {
            let kept = write_atomically(&self.dir.join("segments.json"), |p| {
                fs::write(p, after.to_string()).map_err(|e| file_error("saving", e))
            });
            if kept.is_err() {
                return false;
            }
        }
        let _ = fs::remove_file(&note);
        true
    }

    /// Notes the videos made (an imported package's are its time-lapse's only copy), in place of
    /// the note before only once written whole.
    fn write_segments(&self, k: &Segments) {
        let written = serde_json::to_string(k).map_err(|e| e.to_string()).and_then(|text| {
            write_atomically(&self.dir.join("segments.json"), |p| {
                fs::write(p, text).map_err(|e| file_error("saving", e))
            })
            .map_err(|e| e.to_string())
        });
        if let Err(e) = written {
            log::warn!("keeping the time-lapse's videos: {e}");
            self.failed();
        }
    }

    /// A video at `path` with no note of it moved aside, kept, out of the way of one going there.
    fn set_aside(&self, path: &Path) {
        if !path.exists() {
            return;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("segment.mp4").to_owned();
        let free = (0..).map(|i| self.dir.join(format!("{SET_ASIDE}{i}-{name}"))).find(|p| !p.exists());
        if let Some(aside) = free {
            let _ = fs::rename(path, aside);
        }
    }

    /// Every package video, and the note of them, gone; unless they hold the only copy of their
    /// frames. With no note that holds there is no telling what they hold, so they go aside
    /// instead, out of the way of what is made next.
    fn drop_segments(&self) {
        let note = self.note();
        if note.as_ref().is_some_and(|k| !self.remakeable(k)) {
            return;
        }
        let _ = fs::remove_file(self.dir.join("segments.json"));
        let Ok(entries) = fs::read_dir(&self.dir) else { return };
        for path in entries.filter_map(|e| e.ok()).map(|e| e.path()) {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_owned();
            if name.starts_with("segment-") && name.ends_with(".mp4") {
                if note.is_some() {
                    let _ = fs::remove_file(&path);
                } else {
                    self.set_aside(&path);
                }
            }
        }
    }
}

/// When the file at `path` was last written, if that can be told.
fn written(path: &Path) -> Option<std::time::SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// The frame a frame file is (`frame-000123.jpg`: 123).
fn frame_index(path: &Path) -> Option<u32> {
    let name = path.file_name()?.to_str()?;
    name.strip_prefix("frame-")?.strip_suffix(".jpg")?.parse().ok()
}

/// The frames an MP4 video holds, its picture's size and codec, from its header (its video
/// track's sample sizes, sample description and track header), nothing decoded; none if it isn't
/// one that can be read so.
pub(crate) fn mp4_frames(path: &Path) -> Option<Mp4> {
    let mut file = fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let moov = mp4_box(&mut file, 0, len, b"moov")?;
    // the video track: the one whose handler is 'vide'
    let mut at = moov.0;
    while let Some((start, end, kind)) = mp4_next(&mut file, at, moov.1) {
        at = end;
        if &kind != b"trak" {
            continue;
        }
        let mdia = mp4_box(&mut file, start, end, b"mdia")?;
        let hdlr = mp4_box(&mut file, mdia.0, mdia.1, b"hdlr")?;
        let mut handler = [0u8; 4];
        file.seek(SeekFrom::Start(hdlr.0 + 8)).ok()?;
        file.read_exact(&mut handler).ok()?;
        if &handler != b"vide" {
            continue;
        }
        let minf = mp4_box(&mut file, mdia.0, mdia.1, b"minf")?;
        let stbl = mp4_box(&mut file, minf.0, minf.1, b"stbl")?;
        let stsz = mp4_box(&mut file, stbl.0, stbl.1, b"stsz")?;
        let frames = mp4_u32(&mut file, stsz.0 + 8)?;
        // the first sample description's name: hvc1 or hev1 for HEVC
        let stsd = mp4_box(&mut file, stbl.0, stbl.1, b"stsd")?;
        let mut entry = [0u8; 4];
        file.seek(SeekFrom::Start(stsd.0 + 12)).ok()?;
        file.read_exact(&mut entry).ok()?;
        let hevc = &entry == b"hvc1" || &entry == b"hev1";
        // the track header's size, 16.16 fixed point, at its end
        let tkhd = mp4_box(&mut file, start, end, b"tkhd")?;
        let mut version = [0u8; 1];
        file.seek(SeekFrom::Start(tkhd.0)).ok()?;
        file.read_exact(&mut version).ok()?;
        let size_at = tkhd.0 + if version[0] == 1 { 88 } else { 76 };
        let (w, h) = (mp4_u32(&mut file, size_at)? >> 16, mp4_u32(&mut file, size_at + 4)? >> 16);
        return Some(Mp4 { frames, width: w, height: h, hevc });
    }
    None
}

/// The first box named `kind` among those from `from` to `to`: where its contents start and it
/// ends.
fn mp4_box(file: &mut fs::File, from: u64, to: u64, kind: &[u8; 4]) -> Option<(u64, u64)> {
    let mut at = from;
    while let Some((start, end, k)) = mp4_next(file, at, to) {
        if &k == kind {
            return Some((start, end));
        }
        at = end;
    }
    None
}

/// The box at `at` (before `to`): where its contents start, where it ends, and its name.
fn mp4_next(file: &mut fs::File, at: u64, to: u64) -> Option<(u64, u64, [u8; 4])> {
    if at + 8 > to {
        return None;
    }
    let mut head = [0u8; 8];
    file.seek(SeekFrom::Start(at)).ok()?;
    file.read_exact(&mut head).ok()?;
    let (size, kind) = head.split_at(4);
    let size = u64::from(u32::from_be_bytes(size.try_into().ok()?));
    let kind: [u8; 4] = kind.try_into().ok()?;
    let (start, end) = match size {
        // a 64-bit size follows
        1 => {
            let mut large = [0u8; 8];
            file.read_exact(&mut large).ok()?;
            (at + 16, at.checked_add(u64::from_be_bytes(large))?)
        }
        // to the end
        0 => (at + 8, to),
        _ => (at + 8, at + size),
    };
    (end <= to && start <= end).then_some((start, end, kind))
}

fn mp4_u32(file: &mut fs::File, at: u64) -> Option<u32> {
    let mut bytes = [0u8; 4];
    file.seek(SeekFrom::Start(at)).ok()?;
    file.read_exact(&mut bytes).ok()?;
    Some(u32::from_be_bytes(bytes))
}

/// The frame files in `dir`.
fn frame_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("frame-")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_spread_a_long_time_lapse_over_the_seconds() {
        assert_eq!(time_lapse_picks(5, None), [0, 1, 2, 3, 4]);
        // fewer frames than the seconds take: all of them
        assert_eq!(time_lapse_picks(5, Some(1)), [0, 1, 2, 3, 4]);
        let picks = time_lapse_picks(301, Some(1));
        assert_eq!(picks.len(), 30);
        assert_eq!((picks[0], picks[29]), (0, 300));
        assert!(picks.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn videos_with_no_note_of_them_are_set_aside_not_deleted() {
        let artwork = std::env::temp_dir().join(format!("procraft-unnoted-{}", std::process::id()));
        let _ = fs::remove_dir_all(&artwork);
        let t = TimeLapse::new(artwork.to_string_lossy().into_owned());
        assert!(t.record([255u8, 0, 0, 255].repeat(16), 4, 4));
        let dir = artwork.join("timelapse");
        // a video the note of which was lost: it may be all there is of its frames
        fs::write(dir.join("segment-1.mp4"), b"the only copy").unwrap();
        fs::write(dir.join("segments.json"), b"{ not whole").unwrap();
        let plan = t.segment_plan().expect("a plan");
        assert!(!plan.append);
        assert!(!dir.join("segment-1.mp4").exists(), "out of the way of what is made next");
        assert_eq!(fs::read(dir.join("set-aside-0-segment-1.mp4")).unwrap(), b"the only copy");
        // asked to delete what was recorded, it goes too
        t.purge();
        assert!(!dir.join("set-aside-0-segment-1.mp4").exists());
        let _ = fs::remove_dir_all(&artwork);
    }

    #[test]
    fn a_time_lapse_that_cant_be_kept_is_told_once_until_kept_again() {
        let artwork = std::env::temp_dir().join(format!("procraft-unkept-{}", std::process::id()));
        let _ = fs::remove_dir_all(&artwork);
        fs::create_dir_all(&artwork).unwrap();
        let t = TimeLapse::new(artwork.to_string_lossy().into_owned());
        // a file where its folder goes: nothing can be written
        let dir = artwork.join("timelapse");
        let _ = fs::remove_dir_all(&dir);
        fs::write(&dir, b"in the way").unwrap();
        let red = [255u8, 0, 0, 255].repeat(16);
        let blue = [0u8, 0, 255, 255].repeat(16);
        assert!(!t.record(red.clone(), 4, 4));
        assert!(t.take_problem().is_some(), "told");
        assert!(!t.record(blue.clone(), 4, 4));
        assert_eq!(t.take_problem(), None, "once");
        // room again: kept, and a later failure is told again
        fs::remove_file(&dir).unwrap();
        assert!(t.record(red, 4, 4));
        assert_eq!(t.take_problem(), None);
        fs::remove_dir_all(&dir).unwrap();
        fs::write(&dir, b"in the way").unwrap();
        assert!(!t.record(blue, 4, 4));
        assert!(t.take_problem().is_some(), "told again");
        let _ = fs::remove_file(&dir);
        let _ = fs::remove_dir_all(&artwork);
    }

    #[test]
    fn frames_are_kept_once_on_white_and_counted_again() {
        let dir = std::env::temp_dir().join(format!("procraft-timelapse-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let t = TimeLapse::new(dir.to_string_lossy().into_owned());
        assert_eq!(t.count(), 0);
        // a clear canvas comes out white; the same picture again isn't kept
        let clear = vec![0u8; 4 * 4 * 4];
        assert!(t.record(clear.clone(), 4, 4));
        assert!(!t.record(clear, 4, 4));
        let red = [255u8, 0, 0, 255].repeat(16);
        assert!(t.record(red, 4, 4));
        assert_eq!(t.count(), 2);
        let first = t.frame(0).unwrap();
        assert_eq!((first.width, first.height), (4, 4));
        assert!(first.rgba[..3].iter().all(|&c| c > 240), "{:?}", &first.rgba[..4]);
        let second = t.frame(1).unwrap();
        assert!(second.rgba[0] > 200 && second.rgba[1] < 60);
        assert!(t.frame(2).is_none());
        assert!(t.frame_file(1).is_some_and(|f| f.ends_with("frame-000001.jpg")));
        assert!(t.frame_file(2).is_none());
        // not recording: nothing kept; counted again from the files
        t.set_recording(false);
        assert!(!t.record([0u8, 0, 255, 255].repeat(16), 4, 4));
        assert_eq!(TimeLapse::new(dir.to_string_lossy().into_owned()).count(), 2);
        t.set_recording(true);
        assert!(t.recording());
        let s = TimeLapseSettings { resolution: 2, quality: 3, hevc: true };
        t.set_settings(s);
        assert_eq!(t.settings(), s);
        assert_eq!((time_lapse_side(s), time_lapse_bits_per_pixel(s)), (3840, 16));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn canvas_information_names_the_settings_as_the_canvas_sheet_picks_them() {
        let names =
            |resolution, quality, hevc| time_lapse_setting_names(TimeLapseSettings { resolution, quality, hevc });
        assert_eq!(names(0, 1, false), ["Good", "1080p", "H.264"]);
        assert_eq!(names(2, 3, true), ["Lossless", "4K", "HEVC"]);
        assert_eq!(names(1, 0, false), ["Low", "2K", "H.264"]);
        assert_eq!(names(1, 2, false)[0], "Studio");
    }

    #[test]
    fn purging_deletes_what_was_recorded_and_keeps_the_settings_and_the_switch() {
        let dir = std::env::temp_dir().join(format!("procraft-purge-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let t = TimeLapse::new(dir.to_string_lossy().into_owned());
        let s = TimeLapseSettings { resolution: 1, quality: 2, hevc: false };
        t.set_settings(s);
        assert!(t.record(vec![0u8; 4 * 4 * 4], 4, 4));
        assert!(t.record([255u8, 0, 0, 255].repeat(16), 4, 4));
        t.set_recording(false);
        t.purge();
        assert_eq!(t.count(), 0);
        assert!(t.frame(0).is_none() && t.clips().is_empty());
        assert_eq!(TimeLapse::new(dir.to_string_lossy().into_owned()).count(), 0);
        assert_eq!(t.settings(), s);
        assert!(!t.recording());
        // recording again starts afresh, the first picture kept even if it was the last before
        t.set_recording(true);
        assert!(t.record([255u8, 0, 0, 255].repeat(16), 4, 4));
        assert_eq!(t.count(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn package_videos_are_made_once_and_added_to() {
        let dir = std::env::temp_dir().join(format!("procraft-segments-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let t = TimeLapse::new(dir.to_string_lossy().into_owned());
        assert_eq!(t.segment_plan(), None, "no frames: nothing to make");
        for i in 0..3u8 {
            assert!(t.record([i, 0, 0, 255].repeat(4), 2, 2));
        }
        // the first share makes every frame into one video
        let plan = t.segment_plan().unwrap();
        assert_eq!((plan.frames.clone(), plan.width, plan.append), (vec![0, 1, 2], 0, false));
        // one cut short (no header to read) isn't noted
        fs::write(&plan.path, b"video").unwrap();
        let plan = t.segment_made(plan, 640, 480, true).unwrap_or_else(|| t.segment_plan().unwrap());
        fake_mp4(Path::new(&plan.path), 3, 640, 480);
        assert_eq!(t.segment_made(plan, 640, 480, true), None);
        assert_eq!(t.segment_plan(), None, "all made");
        let p = t.for_package();
        assert_eq!((p.segments.len(), p.width, p.height, p.fps, p.bitrate), (1, 640, 480, 30, 1_228_800));
        // frames since: one more video the same size
        assert!(t.record([9, 9, 9, 255].repeat(4), 2, 2));
        let plan = t.segment_plan().unwrap();
        assert_eq!((plan.frames.clone(), plan.width, plan.height, plan.append), (vec![3], 640, 480, true));
        assert!(plan.path.ends_with("segment-2.mp4"));
        // it couldn't be made that size: all again as one
        let again = t.segment_made(plan, 0, 0, false).unwrap();
        assert_eq!((again.frames.len(), again.append), (4, false));
        assert!(t.for_package().segments.is_empty());
        // other settings: the videos made before go
        fake_mp4(Path::new(&again.path), 4, 640, 480);
        t.segment_made(again, 640, 480, true);
        t.set_settings(TimeLapseSettings { hevc: true, ..t.settings() });
        assert!(!t.segment_plan().unwrap().append);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_package_time_lapse_comes_in_with_its_videos() {
        let dir = std::env::temp_dir().join(format!("procraft-take-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let video = dir.join("brought.mp4");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&video, b"video").unwrap();
        let t = TimeLapse::new(dir.join("artwork").to_string_lossy().into_owned());
        let package = PackageTimeLapse {
            recording: false,
            quality: 2,
            resolution: 1,
            hevc: true,
            segments: vec![video.to_string_lossy().into_owned()],
            width: 640,
            height: 480,
            fps: 30,
            bitrate: 5_000_000,
        };
        t.take_begin(package);
        assert!(!t.recording());
        assert_eq!(t.settings(), TimeLapseSettings { resolution: 1, quality: 2, hevc: true });
        // its video's two frames come in, so the video is kept to go back into a package
        assert!(t.keep_frame([10, 20, 30, 255].repeat(4), 2, 2));
        assert!(t.keep_frame([40, 50, 60, 255].repeat(4), 2, 2));
        t.take_video(video.to_string_lossy().into_owned(), 2);
        t.take_end();
        assert_eq!(t.count(), 2);
        assert_eq!(t.segment_plan(), None, "nothing to make again");
        let p = t.for_package();
        assert_eq!((p.segments.len(), p.width, p.height, p.bitrate), (1, 640, 480, 5_000_000));
        assert!(Path::new(&p.segments[0]).exists());
        // a video whose frames didn't all come in isn't kept
        let t2 = TimeLapse::new(dir.join("other").to_string_lossy().into_owned());
        t2.take_begin(PackageTimeLapse { segments: vec![], ..t.for_package() });
        assert!(t2.keep_frame([0, 0, 0, 255].repeat(4), 2, 2));
        t2.take_video(video.to_string_lossy().into_owned(), 3);
        t2.take_end();
        assert!(t2.for_package().segments.is_empty());
        // nor are those before it, which leave nothing behind
        let third = dir.join("third");
        let t3 = TimeLapse::new(third.to_string_lossy().into_owned());
        t3.take_begin(PackageTimeLapse { segments: vec![], ..t.for_package() });
        assert!(t3.keep_frame([0, 0, 0, 255].repeat(4), 2, 2));
        t3.take_video(video.to_string_lossy().into_owned(), 1);
        t3.take_video(video.to_string_lossy().into_owned(), 2);
        t3.take_end();
        let names = fs::read_dir(&third).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned());
        assert!(!names.into_iter().any(|n| n.ends_with(".mp4")));
        let _ = fs::remove_dir_all(&dir);
    }

    /// An MP4 with only the header boxes read: a video track of `frames` frames, `w` x `h`.
    fn fake_mp4(path: &Path, frames: u32, w: u32, h: u32) {
        fn boxed(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
            let mut b = (u32::try_from(body.len() + 8).unwrap()).to_be_bytes().to_vec();
            b.extend_from_slice(kind);
            b.extend_from_slice(body);
            b
        }
        let mut tkhd = vec![0u8; 84];
        tkhd[76..80].copy_from_slice(&(w << 16).to_be_bytes());
        tkhd[80..84].copy_from_slice(&(h << 16).to_be_bytes());
        let mut hdlr = vec![0u8; 8];
        hdlr.extend_from_slice(b"vide");
        hdlr.extend_from_slice(&[0u8; 13]);
        let mut stsz = vec![0u8; 8];
        stsz.extend_from_slice(&frames.to_be_bytes());
        // one sample description, H.264
        let mut stsd = vec![0, 0, 0, 0, 0, 0, 0, 1];
        stsd.extend_from_slice(&boxed(b"avc1", &[0u8; 8]));
        let stbl = boxed(b"stbl", &[boxed(b"stsd", &stsd), boxed(b"stsz", &stsz)].concat());
        let minf = boxed(b"minf", &stbl);
        let mdia = boxed(b"mdia", &[boxed(b"hdlr", &hdlr), minf].concat());
        let trak = boxed(b"trak", &[boxed(b"tkhd", &tkhd), mdia].concat());
        let file = [boxed(b"ftyp", b"isom"), boxed(b"mdat", &[1, 2, 3]), boxed(b"moov", &trak)].concat();
        fs::write(path, file).unwrap();
    }

    #[test]
    fn an_mp4s_frames_and_size_come_from_its_header() {
        let dir = std::env::temp_dir().join(format!("procraft-mp4-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let video = dir.join("clip.mp4");
        fake_mp4(&video, 241, 672, 1102);
        assert_eq!(mp4_frames(&video), Some(Mp4 { frames: 241, width: 672, height: 1102, hevc: false }));
        fs::write(&video, b"not a video").unwrap();
        assert_eq!(mp4_frames(&video), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_package_time_lapse_comes_in_as_its_videos_and_grows_by_one_more() {
        let dir = std::env::temp_dir().join(format!("procraft-clips-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let brought: Vec<String> = [3u32, 2]
            .iter()
            .enumerate()
            .map(|(i, &n)| {
                let p = dir.join(format!("brought-{i}.mp4"));
                fake_mp4(&p, n, 672, 1102);
                p.to_string_lossy().into_owned()
            })
            .collect();
        let artwork = dir.join("artwork").to_string_lossy().into_owned();
        let t = TimeLapse::new(artwork.clone());
        let package = PackageTimeLapse {
            recording: true,
            quality: 1,
            resolution: 0,
            hevc: true,
            segments: brought,
            // what the package notes, not what its videos are
            width: 640,
            height: 1050,
            fps: 30,
            bitrate: 2_000_000,
        };
        assert!(t.take_clips(package));
        // its videos moved in, their frames counted, no pictures made
        assert_eq!(t.count(), 5);
        assert!(frame_files(&t.dir).is_empty());
        let clips = t.clips();
        assert_eq!(clips.iter().map(|c| (c.first, c.frames)).collect::<Vec<_>>(), [(0, 3), (3, 2)]);
        assert_eq!(TimeLapse::new(artwork.clone()).count(), 5, "counted again from the note");
        let p = t.for_package();
        assert_eq!((p.segments.len(), p.width, p.height, p.bitrate, p.hevc), (2, 640, 1050, 2_000_000, true));
        // other settings don't lose them: they hold the only copy
        t.set_settings(TimeLapseSettings { hevc: false, quality: 3, ..t.settings() });
        assert_eq!(t.segment_plan(), None);
        assert_eq!(t.clips().len(), 2);
        // drawn on: the frames after them become one more video, as those are
        assert!(t.record([9, 9, 9, 255].repeat(4), 2, 2));
        assert_eq!(t.count(), 6);
        assert!(t.frame_file(5).is_some_and(|f| f.ends_with("frame-000005.jpg")));
        let plan = t.segment_plan().unwrap();
        // made as its videos are (H.264, their size), not as the package notes
        assert_eq!(
            (plan.frames.clone(), plan.width, plan.height, plan.append, plan.hevc),
            (vec![5], 672, 1102, true, false)
        );
        // made a size the encoder won't: they stay, the new frame stays a picture
        assert_eq!(t.segment_made(plan.clone(), 0, 0, false), None);
        assert_eq!(t.clips().len(), 2);
        fake_mp4(Path::new(&plan.path), 1, 672, 1102);
        assert_eq!(t.segment_made(plan, 672, 1102, true), None);
        assert_eq!(t.clips().len(), 3);
        // the picture the new video holds goes
        assert_eq!(t.drop_clip_frames(), 1);
        assert!(frame_files(&t.dir).is_empty());
        assert_eq!(TimeLapse::new(artwork).count(), 6);
        // a video whose frames can't be counted: taken in frame by frame instead
        let t2 = TimeLapse::new(dir.join("other").to_string_lossy().into_owned());
        let bad = dir.join("bad.mp4");
        fs::write(&bad, b"video").unwrap();
        assert!(
            !t2.take_clips(PackageTimeLapse { segments: vec![bad.to_string_lossy().into_owned()], ..t.for_package() })
        );
        assert!(bad.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn pictures_go_only_once_their_videos_hold_them() {
        let dir = std::env::temp_dir().join(format!("procraft-dropframes-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let t = TimeLapse::new(dir.to_string_lossy().into_owned());
        for i in 0..3u8 {
            assert!(t.record([i, 0, 0, 255].repeat(4), 2, 2));
        }
        let plan = t.segment_plan().unwrap();
        // the video holds fewer frames than it was made of: not noted, the pictures stay
        fake_mp4(Path::new(&plan.path), 2, 2, 2);
        assert_eq!(t.segment_made(plan.clone(), 2, 2, true), None);
        assert!(t.clips().is_empty() && !Path::new(&plan.path).exists());
        assert_eq!(t.drop_clip_frames(), 0);
        assert_eq!(frame_files(&t.dir).len(), 3);
        // a video noted that then holds fewer (cut short on the disk): its pictures stay
        let plan = t.segment_plan().unwrap();
        fake_mp4(Path::new(&plan.path), 3, 2, 2);
        assert_eq!(t.segment_made(plan, 2, 2, true), None);
        let clip = t.clips().remove(0);
        fake_mp4(Path::new(&clip.path), 2, 2, 2);
        assert_eq!(t.drop_clip_frames(), 0);
        fake_mp4(Path::new(&clip.path), 3, 2, 2);
        assert_eq!(t.drop_clip_frames(), 3);
        assert_eq!((t.count(), TimeLapse::new(dir.to_string_lossy().into_owned()).count()), (3, 3));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_videos_whose_pictures_are_still_there_are_read_back() {
        let dir = std::env::temp_dir().join(format!("procraft-readback-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let t = TimeLapse::new(dir.to_string_lossy().into_owned());
        for i in 0..3u8 {
            assert!(t.record([i, 0, 0, 255].repeat(4), 2, 2));
        }
        assert!(t.clips_with_pictures().is_empty());
        let plan = t.segment_plan().unwrap();
        fake_mp4(Path::new(&plan.path), 3, 2, 2);
        t.segment_made(plan.clone(), 2, 2, true);
        let first = TimeLapseClip { path: t.segment_path(0), first: 0, frames: 3 };
        assert_eq!(t.clips_with_pictures(), [first]);
        // an app that can't read it back leaves them: every frame is still there
        assert_eq!(frame_files(&t.dir).len(), 3);
        assert_eq!(t.drop_clip_frames(), 3);
        assert!(t.clips_with_pictures().is_empty());
        // the next video is the only one to check
        assert!(t.record([9, 0, 0, 255].repeat(4), 2, 2));
        let plan = t.segment_plan().unwrap();
        fake_mp4(Path::new(&plan.path), 1, 2, 2);
        t.segment_made(plan.clone(), 2, 2, true);
        assert_eq!(t.clips_with_pictures(), [TimeLapseClip { path: t.segment_path(1), first: 3, frames: 1 }]);
        let _ = fs::remove_dir_all(&dir);
    }

    /// A folder for a recording as an earlier version left it.
    fn recording(name: &str) -> (PathBuf, PathBuf) {
        let artwork = std::env::temp_dir().join(format!("procraft-layout-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&artwork);
        let dir = artwork.join("timelapse");
        fs::create_dir_all(&dir).unwrap();
        (artwork, dir)
    }

    /// Frame `i` of a recording, a picture as every version keeps it.
    fn picture(dir: &Path, i: u32) {
        let shade = u8::try_from(i % 200).unwrap();
        image::RgbImage::from_pixel(4, 4, image::Rgb([shade, 10, 20]))
            .save(dir.join(format!("frame-{i:06}.jpg")))
            .unwrap();
    }

    fn opened(artwork: &Path) -> Arc<TimeLapse> {
        TimeLapse::new(artwork.to_string_lossy().into_owned())
    }

    fn spans(t: &TimeLapse) -> Vec<(u32, u32)> {
        t.clips().iter().map(|c| (c.first, c.frames)).collect()
    }

    fn note_text(dir: &Path) -> String {
        fs::read_to_string(dir.join("segments.json")).unwrap()
    }

    #[test]
    fn a_recording_in_every_earlier_layout_reads_back() {
        // layout 1 as the first versions wrote it: every frame a picture, a video of the first
        // three for packages, the note without the videos' own size
        let (artwork, dir) = recording("first");
        (0..5).for_each(|i| picture(&dir, i));
        fs::write(dir.join("settings.json"), r#"{"resolution":1,"quality":2,"hevc":false}"#).unwrap();
        fs::write(
            dir.join("segments.json"),
            r#"{"resolution":1,"quality":2,"hevc":false,"width":640,"height":480,"bitrate":1228800,"ends":[3]}"#,
        )
        .unwrap();
        fake_mp4(&dir.join("segment-1.mp4"), 3, 640, 480);
        let t = opened(&artwork);
        assert_eq!(t.count(), 5);
        assert_eq!(spans(&t), [(0, 3)]);
        assert!((0..5).all(|i| t.frame(i).is_some() && t.frame_kept(i)));
        assert_eq!(t.settings(), TimeLapseSettings { resolution: 1, quality: 2, hevc: false });
        // written again in this layout, nothing else of it changed
        assert!(note_text(&dir).contains(r#""version":2"#) && note_text(&dir).contains(r#""ends":[3]"#));
        let plan = t.segment_plan().unwrap();
        assert_eq!((plan.frames.clone(), plan.append, plan.width, plan.noted), (vec![3, 4], true, 640, 1));
        let _ = fs::remove_dir_all(&artwork);

        // layout 1 as 0.0.4 to 0.0.6 left it: the pictures the videos hold gone, the videos' own
        // size and codec noted, recording off, a video set aside
        let (artwork, dir) = recording("videos");
        (7..9).for_each(|i| picture(&dir, i));
        fs::write(dir.join("off"), b"").unwrap();
        fs::write(dir.join("settings.json"), r#"{"resolution":0,"quality":1,"hevc":true}"#).unwrap();
        fs::write(
            dir.join("segments.json"),
            r#"{"resolution":0,"quality":1,"hevc":true,"width":640,"height":1050,"bitrate":6553600,"ends":[4,7],"clip_size":[672,1102],"clip_hevc":false}"#,
        )
        .unwrap();
        fake_mp4(&dir.join("segment-1.mp4"), 4, 672, 1102);
        fake_mp4(&dir.join("segment-2.mp4"), 3, 672, 1102);
        fs::write(dir.join("set-aside-0-segment-1.mp4"), b"kept").unwrap();
        let t = opened(&artwork);
        assert_eq!(t.count(), 9);
        assert_eq!(spans(&t), [(0, 4), (4, 3)]);
        assert!((0..9).all(|i| t.frame_kept(i)));
        assert!(!t.recording());
        assert!(dir.join("set-aside-0-segment-1.mp4").exists());
        let p = t.for_package();
        assert_eq!((p.segments.len(), p.width, p.height, p.hevc), (2, 640, 1050, true));
        let plan = t.segment_plan().unwrap();
        assert_eq!((plan.frames.clone(), plan.width, plan.height, plan.hevc), (vec![7, 8], 672, 1102, false));
        let _ = fs::remove_dir_all(&artwork);

        // this layout, and a later one: read as far as it goes, never refused nor written over
        for (version, extra) in [(TIME_LAPSE_FORMAT, ""), (TIME_LAPSE_FORMAT + 1, r#","frames_in":"elsewhere""#)] {
            let (artwork, dir) = recording(&format!("v{version}"));
            let text = format!(
                r#"{{"version":{version},"resolution":0,"quality":1,"hevc":false,"width":4,"height":4,"bitrate":1000000,"ends":[2],"clip_size":null,"clip_hevc":null{extra}}}"#
            );
            fs::write(dir.join("segments.json"), &text).unwrap();
            fake_mp4(&dir.join("segment-1.mp4"), 2, 4, 4);
            picture(&dir, 2);
            let t = opened(&artwork);
            assert_eq!((t.count(), spans(&t)), (3, vec![(0, 2)]));
            assert_eq!(note_text(&dir), text);
            let _ = fs::remove_dir_all(&artwork);
        }
    }

    #[test]
    fn a_damaged_note_leaves_out_only_what_is_gone() {
        // a video noted that was never made (two makers once noted one each for one file): the
        // note keeps the rest, the frames after are pictures still
        let (artwork, dir) = recording("unmade");
        fs::write(
            dir.join("segments.json"),
            r#"{"resolution":0,"quality":1,"hevc":false,"width":4,"height":4,"bitrate":0,"ends":[3,5,8]}"#,
        )
        .unwrap();
        fake_mp4(&dir.join("segment-1.mp4"), 3, 4, 4);
        fake_mp4(&dir.join("segment-2.mp4"), 2, 4, 4);
        (5..8).for_each(|i| picture(&dir, i));
        let t = opened(&artwork);
        assert_eq!((t.count(), spans(&t)), (8, vec![(0, 3), (3, 2)]));
        assert!((0..8).all(|i| t.frame_kept(i)));
        assert!(note_text(&dir).contains(r#""ends":[3,5]"#));
        let plan = t.segment_plan().unwrap();
        assert_eq!((plan.frames.clone(), plan.append, plan.noted), (vec![5, 6, 7], true, 2));
        let _ = fs::remove_dir_all(&artwork);

        // a video gone from the middle: only its frames are left out
        let (artwork, dir) = recording("hole");
        fs::write(dir.join("segments.json"), r#"{"ends":[3,5,8],"width":4,"height":4}"#).unwrap();
        fake_mp4(&dir.join("segment-1.mp4"), 3, 4, 4);
        fake_mp4(&dir.join("segment-3.mp4"), 3, 4, 4);
        let t = opened(&artwork);
        assert_eq!((t.count(), spans(&t)), (8, vec![(0, 3), (5, 3)]));
        let kept: Vec<bool> = (0..8).map(|i| t.frame_kept(i)).collect();
        assert_eq!(kept, [true, true, true, false, false, true, true, true]);
        assert!(!t.all_in_videos());
        assert_eq!(t.for_package().segments.len(), 2);
        let _ = fs::remove_dir_all(&artwork);

        // a value that can't be read: the videos before it stay; a size not said, the video's
        let (artwork, dir) = recording("value");
        fs::write(dir.join("segments.json"), r#"{"quality":"good","ends":[3,"x",9]}"#).unwrap();
        fake_mp4(&dir.join("segment-1.mp4"), 3, 6, 8);
        fake_mp4(&dir.join("segment-2.mp4"), 6, 6, 8);
        let t = opened(&artwork);
        assert_eq!((t.count(), spans(&t)), (3, vec![(0, 3)]));
        assert_eq!((t.for_package().width, t.for_package().height), (6, 8));
        let _ = fs::remove_dir_all(&artwork);

        // a frame missing between pictures, and one half written when the app was stopped: the
        // next is recorded after them all, none written over
        let (artwork, dir) = recording("gap");
        [0, 1, 3].into_iter().for_each(|i| picture(&dir, i));
        let half = dir.join("frame-000004.jpg.part");
        fs::write(&half, b"\xff\xd8 cut short").unwrap();
        let long_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(600);
        fs::File::options().write(true).open(&half).unwrap().set_modified(long_ago).unwrap();
        let t = opened(&artwork);
        assert!(!half.exists());
        assert_eq!(t.count(), 4);
        assert!(!t.frame_kept(2) && t.frame(2).is_none());
        let before = fs::read(dir.join("frame-000003.jpg")).unwrap();
        assert!(t.record([1u8, 2, 3, 255].repeat(16), 4, 4));
        assert!(t.frame_file(4).is_some());
        assert_eq!(fs::read(dir.join("frame-000003.jpg")).unwrap(), before);
        // nothing half written left by a frame kept
        assert!(frame_files(&dir).iter().all(|f| frame_index(f).is_some()));
        let _ = fs::remove_dir_all(&artwork);
    }

    #[test]
    fn videos_whose_note_was_lost_are_taken_back_where_their_frames_are_nowhere_else() {
        // no note: the videos hold the frames before the pictures
        let (artwork, dir) = recording("lost");
        fake_mp4(&dir.join("segment-1.mp4"), 3, 4, 4);
        fake_mp4(&dir.join("segment-2.mp4"), 2, 4, 4);
        (5..7).for_each(|i| picture(&dir, i));
        let t = opened(&artwork);
        assert_eq!((t.count(), spans(&t)), (7, vec![(0, 3), (3, 2)]));
        assert!(note_text(&dir).contains(r#""ends":[3,5]"#));
        let _ = fs::remove_dir_all(&artwork);

        // set aside by an earlier version once their note was lost: back in
        let (artwork, dir) = recording("aside");
        fake_mp4(&dir.join("set-aside-0-segment-1.mp4"), 3, 4, 4);
        fake_mp4(&dir.join("set-aside-0-segment-2.mp4"), 2, 4, 4);
        fs::write(dir.join("segment-1.mp4"), b"cut short, no header").unwrap();
        let t = opened(&artwork);
        assert_eq!((t.count(), spans(&t)), (5, vec![(0, 3), (3, 2)]));
        assert!(!dir.join("set-aside-0-segment-2.mp4").exists());
        // what was in the way kept too
        let names: Vec<String> =
            fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        assert!(names.iter().any(|n| n.starts_with(SET_ASIDE)), "{names:?}");
        let _ = fs::remove_dir_all(&artwork);

        // a video made but not noted before the app was stopped: its frames are all pictures
        let (artwork, dir) = recording("unnoted");
        (0..3).for_each(|i| picture(&dir, i));
        fake_mp4(&dir.join("segment-1.mp4"), 3, 4, 4);
        let t = opened(&artwork);
        assert_eq!((t.count(), spans(&t)), (3, vec![]));
        let _ = fs::remove_dir_all(&artwork);

        // no video that can be read: nothing guessed, the pictures as they are
        let (artwork, dir) = recording("nothing");
        fs::write(dir.join("set-aside-0-segment-1.mp4"), b"no header").unwrap();
        picture(&dir, 2);
        let t = opened(&artwork);
        assert_eq!((t.count(), spans(&t)), (3, vec![]));
        assert!(dir.join("set-aside-0-segment-1.mp4").exists());
        assert!(!t.frame_kept(0) && t.frame_kept(2));
        let _ = fs::remove_dir_all(&artwork);
    }

    /// The red of frame `i` read back: [`picture`] keeps its number in it.
    fn red(t: &TimeLapse, i: u32) -> i32 {
        i32::from(t.frame(i).unwrap().rgba[0])
    }

    #[test]
    fn a_time_lapse_whose_note_two_makers_broke_comes_back_in_order() {
        // as an earlier version left it: the note voided (a video of the same frames noted twice),
        // so the next close set every video aside, and the frames since were numbered from the
        // pictures there were (two, written before), from 2
        let (artwork, dir) = recording("broken");
        let written = |name: &str, minutes: u64| {
            let when = std::time::SystemTime::now() - std::time::Duration::from_secs(3600 - minutes * 60);
            fs::File::options().write(true).open(dir.join(name)).unwrap().set_modified(when).unwrap();
        };
        let at = |i: u32, minutes: u64| {
            picture(&dir, i);
            written(&format!("frame-{i:06}.jpg"), minutes);
        };
        // a picture the second video holds too (recorded before it was made, its number in it)
        at(4, 0);
        at(5, 1);
        at(6, 2);
        fake_mp4(&dir.join("set-aside-0-segment-1.mp4"), 3, 4, 4);
        fake_mp4(&dir.join("set-aside-0-segment-2.mp4"), 2, 4, 4);
        written("set-aside-0-segment-1.mp4", 3);
        written("set-aside-0-segment-2.mp4", 3);
        at(2, 10);
        at(3, 11);
        let t = opened(&artwork);
        // the videos first, then the pictures in the order they were recorded; the one a video
        // holds where it was
        assert_eq!((t.count(), spans(&t)), (9, vec![(0, 3), (3, 2)]));
        assert!((red(&t, 4) - 4).abs() <= 2);
        let reds: Vec<i32> = (5..9).map(|i| red(&t, i)).collect();
        assert!(reds.iter().zip([5, 6, 2, 3]).all(|(r, was)| (r - was).abs() <= 2), "{reds:?}");
        assert!(!dir.join(RENUMBERING).exists());
        // the next frame after them all
        assert!(t.record([200u8, 0, 0, 255].repeat(16), 4, 4));
        assert!(t.frame_file(9).is_some());
        let _ = fs::remove_dir_all(&artwork);

        // a renumbering cut short (the app stopped mid-way) is finished the next time
        let (artwork, dir) = recording("cutshort");
        fake_mp4(&dir.join("set-aside-0-segment-1.mp4"), 4, 4, 4);
        [0u32, 1].into_iter().for_each(|i| picture(&dir, i + 1));
        fs::write(dir.join(RENUMBERING), r#"{"moves":[[1,4],[2,5]],"placing":false}"#).unwrap();
        fs::rename(dir.join("frame-000001.jpg"), dir.join("frame-000004.jpg.renumbered")).unwrap();
        let t = opened(&artwork);
        assert_eq!((t.count(), spans(&t)), (6, vec![(0, 4)]));
        assert!((red(&t, 4) - 1).abs() <= 2 && (red(&t, 5) - 2).abs() <= 2);
        assert!(!dir.join(RENUMBERING).exists());
        let _ = fs::remove_dir_all(&artwork);

        // the note itself, the same video noted twice: read up to it
        let (artwork, dir) = recording("twicenoted");
        fs::write(dir.join("segments.json"), r#"{"width":4,"height":4,"ends":[3,5,5]}"#).unwrap();
        fake_mp4(&dir.join("segment-1.mp4"), 3, 4, 4);
        fake_mp4(&dir.join("segment-2.mp4"), 2, 4, 4);
        let t = opened(&artwork);
        assert_eq!((t.count(), spans(&t)), (5, vec![(0, 3), (3, 2)]));
        let _ = fs::remove_dir_all(&artwork);
    }

    #[test]
    fn two_makers_of_the_same_frames_note_one_video_of_them() {
        // the artwork closed (its frames being made into a video) and opened and closed again
        // meanwhile: two objects on one folder
        let (artwork, _) = recording("twice");
        let first = opened(&artwork);
        for i in 0..3u8 {
            assert!(first.record([i, 0, 0, 255].repeat(4), 2, 2));
        }
        let plan = first.segment_plan().unwrap();
        let again = opened(&artwork);
        assert!(again.record([9, 9, 9, 255].repeat(4), 2, 2));
        let other = again.segment_plan().unwrap();
        assert_ne!(plan.path, other.path, "each made in a file of its own");
        fake_mp4(Path::new(&plan.path), 3, 2, 2);
        assert_eq!(first.segment_made(plan, 2, 2, true), None);
        // the second, made of frames now noted: it goes, what is left is planned again
        fake_mp4(Path::new(&other.path), 4, 2, 2);
        let rest = again.segment_made(other.clone(), 2, 2, true).unwrap();
        assert!(!Path::new(&other.path).exists());
        assert_eq!((rest.frames.clone(), rest.append, rest.noted), (vec![3], true, 1));
        fake_mp4(Path::new(&rest.path), 1, 2, 2);
        assert_eq!(again.segment_made(rest, 2, 2, true), None);
        assert_eq!(spans(&again), [(0, 3), (3, 1)]);
        assert_eq!((opened(&artwork).count(), again.segment_plan()), (4, None));
        assert!(again.all_in_videos());
        assert_eq!(again.drop_clip_frames(), 4);
        assert!((0..4).all(|i| again.frame_kept(i)));
        let _ = fs::remove_dir_all(&artwork);
    }

    #[test]
    fn the_note_is_written_in_this_layout() {
        let k = Segments {
            version: TIME_LAPSE_FORMAT,
            resolution: 0,
            quality: 1,
            hevc: false,
            width: 640,
            height: 480,
            bitrate: 1_228_800,
            ends: vec![3, 5],
            clip_size: Some((672, 1102)),
            clip_hevc: Some(false),
        };
        // a change here is a new layout: give TIME_LAPSE_FORMAT the next number, and keep a reader
        // of this one and a recording in it in `a_recording_in_every_earlier_layout_reads_back`
        assert_eq!(
            serde_json::to_string(&k).unwrap(),
            r#"{"version":2,"resolution":0,"quality":1,"hevc":false,"width":640,"height":480,"bitrate":1228800,"ends":[3,5],"clip_size":[672,1102],"clip_hevc":false}"#
        );
        assert_eq!(Segments::read(&serde_json::to_string(&k).unwrap()), Some(k));
    }
    #[test]
    fn a_big_time_lapse_reads_back_whole() {
        // 40 videos (more than are made again as one), over 18,000 frames in them and pictures after,
        // one past a million
        let (artwork, dir) = recording("big");
        let sizes: Vec<u32> = (0..40).map(|i| 400 + i * 3).collect();
        let mut end = 0;
        let mut ends = Vec::new();
        for (i, &n) in sizes.iter().enumerate() {
            fake_mp4(&dir.join(format!("segment-{}.mp4", i + 1)), n, 1280, 1280);
            end += n;
            ends.push(end);
        }
        fs::write(
            dir.join("segments.json"),
            format!(r#"{{"resolution":0,"quality":1,"hevc":false,"width":1280,"height":1280,"bitrate":6553600,"ends":{ends:?}}}"#),
        )
        .unwrap();
        (end..end + 3).for_each(|i| picture(&dir, i));
        let t = opened(&artwork);
        assert_eq!((t.count(), t.clips().len()), (end + 3, 40));
        assert!([0, 399, 400, end - 1, end + 2].iter().all(|&i| t.frame_kept(i)));
        // the frames since become one more video, added to the 40 (they hold the only copy)
        let plan = t.segment_plan().unwrap();
        assert_eq!((plan.frames.len(), plan.append, plan.noted), (3, true, 40));
        fake_mp4(Path::new(&plan.path), 3, 1280, 1280);
        assert_eq!(t.segment_made(plan, 1280, 1280, true), None);
        assert_eq!(t.drop_clip_frames(), 3);
        assert_eq!((opened(&artwork).count(), t.clips().len(), t.all_in_videos()), (end + 3, 41, true));
        // frame numbers past six digits
        picture(&dir, 1_234_567);
        let t = opened(&artwork);
        assert_eq!(t.count(), 1_234_568);
        assert!(t.frame(1_234_567).is_some());
        let _ = fs::remove_dir_all(&artwork);
    }

    #[test]
    fn an_mp4_with_64_bit_box_sizes_is_counted() {
        // the boxes as a writer puts them past 4 GB: a size of 1 and the size after the name
        let dir = std::env::temp_dir().join(format!("procraft-mp4-64-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let small = dir.join("small.mp4");
        fake_mp4(&small, 7, 640, 480);
        let bytes = fs::read(&small).unwrap();
        let mut large = Vec::new();
        let mut at = 0;
        while at + 8 <= bytes.len() {
            let size = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
            large.extend_from_slice(&1u32.to_be_bytes());
            large.extend_from_slice(&bytes[at + 4..at + 8]);
            large.extend_from_slice(&((size + 8) as u64).to_be_bytes());
            large.extend_from_slice(&bytes[at + 8..at + size]);
            at += size;
        }
        let video = dir.join("large.mp4");
        fs::write(&video, large).unwrap();
        assert_eq!(mp4_frames(&video).map(|m| m.frames), Some(7));
        let _ = fs::remove_dir_all(&dir);
    }
    #[test]
    fn recordings_as_0_0_5_left_them_read_back() {
        // as that version's own code wrote them (its makers run as the app runs them), file for
        // file: frames written in this order, videos with these frames, these notes
        let (artwork, dir) = recording("left-plain");
        let note = r#"{"resolution":0,"quality":1,"hevc":false,"width":2,"height":2,"bitrate":1000000,"ends":[5,8],"clip_size":null,"clip_hevc":null}"#;
        fs::write(dir.join("segments.json"), note).unwrap();
        fake_mp4(&dir.join("segment-1.mp4"), 5, 2, 2);
        fake_mp4(&dir.join("segment-2.mp4"), 3, 2, 2);
        (8..10).for_each(|i| picture(&dir, i));
        let t = opened(&artwork);
        assert_eq!((t.count(), spans(&t)), (10, vec![(0, 5), (5, 3)]));
        assert!((0..10).all(|i| t.frame_kept(i)));
        let _ = fs::remove_dir_all(&artwork);

        // the artwork closed while its last session's frames were being made into a video, and
        // opened, drawn on and closed again meanwhile: the same frames noted twice, a video noted
        // that was never made; that version read it as 3 frames, none of them there
        let (artwork, dir) = recording("left-race");
        let note = r#"{"resolution":0,"quality":1,"hevc":false,"width":2,"height":2,"bitrate":1000000,"ends":[3,7,10],"clip_size":null,"clip_hevc":null}"#;
        fs::write(dir.join("segments.json"), note).unwrap();
        fake_mp4(&dir.join("segment-1.mp4"), 3, 2, 2);
        fake_mp4(&dir.join("segment-2.mp4"), 7, 2, 2);
        (7..10).for_each(|i| picture(&dir, i));
        let t = opened(&artwork);
        assert_eq!((t.count(), spans(&t)), (10, vec![(0, 3), (3, 4)]));
        assert!((0..10).all(|i| t.frame_kept(i)));
        let _ = fs::remove_dir_all(&artwork);

        // then opened again: its videos set aside at the next close, the frames since numbered
        // from the 3 it counted
        let (artwork, dir) = recording("left-aside");
        let written = |name: &str, seconds: u64| {
            let when = std::time::SystemTime::now() - std::time::Duration::from_secs(600 - seconds);
            fs::File::options().write(true).open(dir.join(name)).unwrap().set_modified(when).unwrap();
        };
        fake_mp4(&dir.join("set-aside-0-segment-1.mp4"), 3, 2, 2);
        written("set-aside-0-segment-1.mp4", 0);
        for (i, at) in [(7, 10), (8, 11), (9, 12)] {
            picture(&dir, i);
            written(&format!("frame-{i:06}.jpg"), at);
        }
        fake_mp4(&dir.join("set-aside-0-segment-2.mp4"), 7, 2, 2);
        written("set-aside-0-segment-2.mp4", 20);
        for (i, at) in [(3, 30), (4, 31), (5, 32), (6, 33)] {
            picture(&dir, i);
            written(&format!("frame-{i:06}.jpg"), at);
        }
        let t = opened(&artwork);
        // the videos back, the frames they hold where they were, the frames since after them
        assert_eq!((t.count(), spans(&t)), (14, vec![(0, 3), (3, 7)]));
        assert!((0..14).all(|i| t.frame_kept(i)));
        let reds: Vec<i32> = (7..14).map(|i| red(&t, i)).collect();
        assert!(reds.iter().zip([7, 8, 9, 3, 4, 5, 6]).all(|(r, was)| (r - was).abs() <= 2), "{reds:?}");
        // and stays so
        assert_eq!((opened(&artwork).count(), spans(&opened(&artwork))), (14, vec![(0, 3), (3, 7)]));
        let _ = fs::remove_dir_all(&artwork);

        // big: 40 sessions of 500 frames, each closed, 7 frames since
        let (artwork, dir) = recording("left-big");
        let ends: Vec<u32> = (1..=40).map(|i| i * 500).collect();
        let note = format!(
            r#"{{"resolution":0,"quality":1,"hevc":false,"width":2,"height":2,"bitrate":1000000,"ends":{ends:?},"clip_size":null,"clip_hevc":null}}"#
        );
        fs::write(dir.join("segments.json"), note.replace(' ', "")).unwrap();
        (1..=40).for_each(|i| fake_mp4(&dir.join(format!("segment-{i}.mp4")), 500, 2, 2));
        (20_000..20_007).for_each(|i| picture(&dir, i));
        let t = opened(&artwork);
        assert_eq!((t.count(), t.clips().len()), (20_007, 40));
        let _ = fs::remove_dir_all(&artwork);
    }

    #[test]
    fn videos_an_earlier_version_set_aside_and_went_on_without_come_back_in_front() {
        // as 0.0.5 left it on a phone: three videos noted twice, so set aside at the next close;
        // a new note from frame 0 with one video of the frames since, and two pictures after
        let (artwork, dir) = recording("before");
        let written = |name: &str, minutes: u64| {
            let when = std::time::SystemTime::now() - std::time::Duration::from_secs(3600 - minutes * 60);
            fs::File::options().write(true).open(dir.join(name)).unwrap().set_modified(when).unwrap();
        };
        for (i, frames, at) in [(1, 5, 0), (2, 418, 3), (3, 634, 6)] {
            let name = format!("set-aside-0-segment-{i}.mp4");
            fake_mp4(&dir.join(&name), frames, 1280, 1280);
            written(&name, at);
        }
        fake_mp4(&dir.join("segment-1.mp4"), 3, 1280, 1280);
        written("segment-1.mp4", 8);
        fs::write(
            dir.join("segments.json"),
            r#"{"resolution":0,"quality":1,"hevc":false,"width":1280,"height":1280,"bitrate":6553600,"ends":[3],"clip_size":null,"clip_hevc":null}"#,
        )
        .unwrap();
        for (i, at) in [(3, 9), (4, 10)] {
            picture(&dir, i);
            written(&format!("frame-{i:06}.jpg"), at);
        }
        let t = opened(&artwork);
        assert_eq!((t.count(), spans(&t)), (1062, vec![(0, 5), (5, 418), (423, 634), (1057, 3)]));
        assert!((0..1062).all(|i| t.frame_kept(i)));
        assert!((red(&t, 1060) - 3).abs() <= 2 && (red(&t, 1061) - 4).abs() <= 2);
        assert!(!dir.join(RENUMBERING).exists() && !dir.join("set-aside-0-segment-1.mp4").exists());
        // and stays so; the next video after them
        let t = opened(&artwork);
        assert_eq!((t.count(), t.clips().len()), (1062, 4));
        let plan = t.segment_plan().unwrap();
        assert_eq!((plan.frames.clone(), plan.noted), (vec![1060, 1061], 4));
        let _ = fs::remove_dir_all(&artwork);

        // the same cut short (the app stopped mid-way): the picture and two videos moved, the
        // last not, the note not yet written; finished the next time
        let (artwork, dir) = recording("before-cut");
        fake_mp4(&dir.join("segment-1.mp4"), 4, 2, 2);
        fake_mp4(&dir.join("segment-3.mp4"), 2, 2, 2);
        fake_mp4(&dir.join("set-aside-0-segment-2.mp4"), 2, 2, 2);
        fs::write(dir.join("segments.json"), r#"{"width":2,"height":2,"ends":[2]}"#).unwrap();
        picture(&dir, 8);
        fs::write(
            dir.join(RENUMBERING),
            r#"{"moves":[[2,8]],"placing":true,"videos":[["segment-1.mp4","segment-3.mp4"],["set-aside-0-segment-1.mp4","segment-1.mp4"],["set-aside-0-segment-2.mp4","segment-2.mp4"]],"note":{"version":2,"resolution":0,"quality":1,"hevc":false,"width":2,"height":2,"bitrate":0,"ends":[4,6,8],"clip_size":null,"clip_hevc":null}}"#,
        )
        .unwrap();
        let t = opened(&artwork);
        assert_eq!((t.count(), spans(&t)), (9, vec![(0, 4), (4, 2), (6, 2)]));
        assert!(!dir.join(RENUMBERING).exists());
        let _ = fs::remove_dir_all(&artwork);

        // one video alone set aside (a close stopped mid-way), its frames in the one noted: left
        let (artwork, dir) = recording("leftover");
        fake_mp4(&dir.join("set-aside-0-segment-1.mp4"), 3, 2, 2);
        fake_mp4(&dir.join("segment-1.mp4"), 5, 2, 2);
        fs::write(dir.join("segments.json"), r#"{"width":2,"height":2,"ends":[5]}"#).unwrap();
        let t = opened(&artwork);
        assert_eq!((t.count(), spans(&t)), (5, vec![(0, 5)]));
        assert!(dir.join("set-aside-0-segment-1.mp4").exists());
        let _ = fs::remove_dir_all(&artwork);
    }
}
