//! The app's GPU device: one for every artwork, as making one compiles the canvases' pipelines (a
//! second and a half on a tablet).
//!
//! And which graphics backend it is made from (Android: Vulkan, then OpenGL ES). A driver that
//! fails says so and the next backend is tried at once, but one that crashes takes the app down
//! with it, leaving nothing to see. So the app keeps a record (in its own folder) of the backend
//! it is trying while that is at risk: while the device and its pipelines are made, and until a
//! canvas first shows on it. A start that finds a try never finished begins from the next
//! backend; one that drew a canvas is kept for good, until the app or the system is updated: new
//! shaders to compile or a new driver can crash where the old ones drew, and a backend kept for
//! good would then crash every start.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use procraft_gpu::Gpu;

static DEVICE: Mutex<Option<Gpu>> = Mutex::new(None);

/// The backend the app's device was made from (`usize::MAX`: none yet).
static BACKEND: AtomicUsize = AtomicUsize::new(usize::MAX);

/// What is left to the system whatever the app asks for, bytes: GPU memory is charged to no
/// process, so the system can't tell who to stop when it runs out, and stops everything else first.
const LEFT_FREE: u64 = 512 << 20;

/// Less than this, bytes, is never what leaves the system short.
const SMALL_ASK: u64 = 32 << 20;

/// Whether `bytes` more of the GPU's memory would leave the system short. Only where the system
/// says what it has free (Android, whose GPU memory is the same memory as everything else's).
pub(crate) fn short_of_memory(bytes: u64) -> bool {
    bytes >= SMALL_ASK && memory_free().is_some_and(|free| bytes.saturating_add(LEFT_FREE) > free)
}

#[cfg(test)]
std::thread_local! {
    /// What this thread's tests have free, in place of the system's.
    pub(crate) static MEMORY_FREE: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
fn memory_free() -> Option<u64> {
    MEMORY_FREE.with(std::cell::Cell::get)
}

#[cfg(all(target_os = "android", not(test)))]
fn memory_free() -> Option<u64> {
    available_in(&std::fs::read_to_string("/proc/meminfo").ok()?)
}

#[cfg(not(any(target_os = "android", test)))]
fn memory_free() -> Option<u64> {
    None
}

/// The memory `/proc/meminfo`'s text says is available, bytes.
#[cfg(any(target_os = "android", test))]
pub(crate) fn available_in(meminfo: &str) -> Option<u64> {
    let line = meminfo.lines().find_map(|l| l.strip_prefix("MemAvailable:"))?;
    let kb: u64 = line.trim().strip_suffix("kB")?.trim().parse().ok()?;
    Some(kb * 1024)
}

/// Where the record of the backend being tried is kept (the app's own folder), and the build of
/// the app and system it is kept for (see [`set_gpu_record_folder`]); none, none kept.
static RECORD_FOLDER: Mutex<Option<(PathBuf, String)>> = Mutex::new(None);

/// The record's file in [`RECORD_FOLDER`]: "<backend> trying|made|drew", and on a second line the
/// build that wrote it.
pub(crate) const RECORD: &str = "gpu-start.txt";

/// Whether a canvas has shown in this run (so nothing is at risk any more).
#[cfg_attr(not(any(target_os = "android", target_os = "ios")), allow(dead_code))]
static SHOWN: AtomicBool = AtomicBool::new(false);

/// The app's device, held while one is made so that two aren't made at once.
pub(crate) fn kept() -> MutexGuard<'static, Option<Gpu>> {
    DEVICE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `gpu`, a device the app made for itself, is the one for drawing with no window as well.
pub fn keep_device(gpu: &Gpu) {
    *kept() = Some(gpu.clone());
}

/// Makes the app's device and its canvases' pipelines ahead of the first artwork, so that it opens
/// at once; nothing if it is made. Slow: call it off the main thread.
#[uniffi::export]
pub fn warm_gpu() {
    start_log();
    let mut kept = kept();
    if kept.is_some() {
        return;
    }
    let from = first_backend();
    let made = std::panic::catch_unwind(|| {
        Gpu::for_canvases_from(from, trying).map(|(gpu, backend)| {
            // the last backend tried is kept whatever its pipelines did: made now if not yet
            gpu.prepare_canvases();
            (gpu, backend)
        })
    });
    match made {
        Ok(Ok((gpu, backend))) => {
            made_on(backend);
            *kept = Some(gpu);
        }
        Ok(Err(e)) => log::error!("making the GPU device: {e}"),
        Err(panic) => log::error!("making the GPU device panicked: {}", procraft_gpu::panic_message(panic.as_ref())),
    }
}

/// Keeps the record of which graphics backend the app is trying in `dir` (the app's own folder),
/// so that a driver crashing the app as its device is made, or before a canvas first shows, sends
/// the next start to the next backend. `build` names the app as installed and the system it runs
/// on, and differs once either is updated: a backend that drew a canvas is trusted only under the
/// build it drew in. Call it before [`warm_gpu`].
#[uniffi::export]
pub fn set_gpu_record_folder(dir: String, build: String) {
    start_log();
    // the canvases' pipelines kept there between runs as well, where the GPU can
    procraft_gpu::keep_pipelines_in(PathBuf::from(&dir));
    *RECORD_FOLDER.lock().unwrap_or_else(PoisonError::into_inner) = Some((PathBuf::from(dir), build));
}

/// The log, to Android's (tag "procraft") and to the app's log file once that has begun (see
/// `app_log`), with a panic's message and place in it (the platform drops what the standard error
/// says); once.
pub(crate) fn start_log() {
    #[cfg(target_os = "android")]
    {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            crate::app_log::install(android_logger::AndroidLogger::new(
                android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("procraft"),
            ));
            let before = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                log::error!("{info}");
                before(info);
            }));
        });
    }
}

/// The backend to start from: the next one after a try that never finished (see the module's
/// note), else the one used last.
pub(crate) fn first_backend() -> usize {
    record().map_or(0, |(dir, _)| start_in(&dir, Gpu::backend_count()))
}

/// Backend `backend` is about to be tried.
pub(crate) fn trying(backend: usize) {
    note(backend, Stage::Trying);
}

/// The app's device is made, from backend `backend`.
pub(crate) fn made_on(backend: usize) {
    BACKEND.store(backend, Ordering::Relaxed);
    note(backend, Stage::Made);
}

/// A canvas is about to show on the app's device: until it has ([`Showing::shown`]), a crash
/// counts against the device's backend. The try ends when this is dropped (unless panicking,
/// which counts against it too).
#[cfg_attr(not(any(target_os = "android", target_os = "ios")), allow(dead_code))]
pub(crate) struct Showing(Option<usize>);

#[cfg_attr(not(any(target_os = "android", target_os = "ios")), allow(dead_code))]
impl Showing {
    pub(crate) fn begin() -> Showing {
        let backend = BACKEND.load(Ordering::Relaxed);
        if SHOWN.load(Ordering::Relaxed) || backend == usize::MAX {
            return Showing(None);
        }
        note(backend, Stage::Trying);
        Showing(Some(backend))
    }

    /// The canvas has shown (a frame presented): its backend draws.
    pub(crate) fn shown(&mut self) {
        if let Some(backend) = self.0.take()
            && !SHOWN.swap(true, Ordering::Relaxed)
        {
            note(backend, Stage::Drew);
        }
    }
}

impl Drop for Showing {
    fn drop(&mut self) {
        // stopped before it showed (the app away), or failed with an error: back as it was
        if let Some(backend) = self.0
            && !std::thread::panicking()
        {
            note(backend, Stage::Made);
        }
    }
}

/// How far a backend got.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    /// Being tried: a device made from it, its pipelines, or a canvas's first frame.
    Trying,
    /// The app's device is made from it.
    Made,
    /// A canvas has shown on it: kept for good.
    Drew,
}

impl Stage {
    fn word(self) -> &'static str {
        match self {
            Stage::Trying => "trying",
            Stage::Made => "made",
            Stage::Drew => "drew",
        }
    }
}

/// The record's folder and the build it is kept for, if it is kept.
fn record() -> Option<(PathBuf, String)> {
    RECORD_FOLDER.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

fn note(backend: usize, stage: Stage) {
    if let Some((dir, build)) = record() {
        note_in(&dir, &build, backend, stage);
    }
}

/// Records in `dir`, for `build`, that `backend` got to `stage`, unless it has drawn already in
/// the same build.
fn note_in(dir: &Path, build: &str, backend: usize, stage: Stage) {
    let path = dir.join(RECORD);
    let saved = std::fs::read_to_string(&path).unwrap_or_default();
    let build = build.trim();
    if read(&saved) == Some((backend, Stage::Drew)) && built_by(&saved) == build {
        return;
    }
    let text = if build.is_empty() {
        format!("{backend} {}", stage.word())
    } else {
        format!("{backend} {}\n{build}", stage.word())
    };
    let written = crate::project::write_atomically(&path, |tmp| {
        std::fs::write(tmp, &text).map_err(|e| crate::project::file_error("the GPU record", e))
    });
    if let Err(e) = written {
        log::warn!("{e}");
    }
}

/// The backend to start from, of `count`, by the record in `dir`.
fn start_in(dir: &Path, count: usize) -> usize {
    let saved = std::fs::read_to_string(dir.join(RECORD)).unwrap_or_default();
    let start = next_start(&saved, count);
    if let Some((backend, Stage::Trying)) = read(&saved) {
        log::warn!("graphics backend {backend} never finished its try (the app crashed): starting from {start}");
    }
    start
}

/// A record's backend and stage (its first line); none if it can't be read.
fn read(saved: &str) -> Option<(usize, Stage)> {
    let mut words = saved.lines().next()?.split_whitespace();
    let backend = words.next()?.parse().ok()?;
    let stage = match words.next()? {
        "trying" => Stage::Trying,
        "made" => Stage::Made,
        "drew" => Stage::Drew,
        _ => return None,
    };
    Some((backend, stage))
}

/// The build that wrote the record `saved` (its second line; empty in a record from before builds
/// were kept, or kept with none).
fn built_by(saved: &str) -> &str {
    saved.split_once('\n').map_or("", |(_, build)| build.trim())
}

/// The backend to start from, of `count`, after the record `saved`: the next one if its try
/// never finished, else the same; the first without a record.
fn next_start(saved: &str, count: usize) -> usize {
    let count = count.max(1);
    match read(saved) {
        Some((backend, Stage::Trying)) => (backend + 1) % count,
        Some((backend, _)) if backend < count => backend,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warming_makes_the_apps_device_once() {
        warm_gpu();
        let Some(first) = kept().clone() else { return eprintln!("skipped: no GPU") };
        warm_gpu();
        let again = kept().clone().unwrap();
        assert!(first.device == again.device);
    }

    /// An artwork exported from the gallery, never shown, draws on the app's own device (on the
    /// backend chosen for the machine), not on one made from whichever backend comes first.
    #[test]
    fn an_artwork_exported_unseen_draws_on_the_apps_device() {
        warm_gpu();
        let Some(app) = kept().clone() else { return eprintln!("skipped: no GPU") };
        let s = crate::CanvasSession::new(64, 64);
        assert!(s.prepare_offscreen());
        assert!(s.shared.lock().gpu.as_ref().is_some_and(|gpu| gpu.device == app.device));
    }

    #[test]
    fn a_backend_whose_try_never_finished_gives_way_to_the_next() {
        assert_eq!(next_start("", 2), 0);
        assert_eq!(next_start("0 made", 2), 0);
        assert_eq!(next_start("1 drew", 2), 1);
        // a crash while trying the first: the second; while trying the last: round to the first
        assert_eq!(next_start("0 trying", 2), 1);
        assert_eq!(next_start("1 trying", 2), 0);
        // fewer backends now (one named by WGPU_BACKEND), or a record it can't read
        assert_eq!(next_start("1 made", 1), 0);
        assert_eq!(next_start("1 trying", 1), 0);
        assert_eq!(next_start("garbled", 2), 0);
        assert_eq!(next_start("0 sideways", 2), 0);
    }

    #[test]
    fn the_record_follows_the_tries_and_keeps_a_backend_that_drew() {
        let dir = std::env::temp_dir().join(format!("procraft-gpu-record-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::fs::remove_file(dir.join(RECORD));
        assert_eq!(start_in(&dir, 2), 0);
        // the first made its device; then crashed before a canvas showed
        note_in(&dir, "", 0, Stage::Trying);
        note_in(&dir, "", 0, Stage::Made);
        assert_eq!(start_in(&dir, 2), 0);
        note_in(&dir, "", 0, Stage::Trying);
        assert_eq!(start_in(&dir, 2), 1);
        // the second draws, and stays so
        note_in(&dir, "", 1, Stage::Trying);
        note_in(&dir, "", 1, Stage::Made);
        note_in(&dir, "", 1, Stage::Drew);
        note_in(&dir, "", 1, Stage::Trying);
        assert_eq!(std::fs::read_to_string(dir.join(RECORD)).unwrap(), "1 drew");
        assert_eq!(start_in(&dir, 2), 1);
        // a crash on the other, tried since (the one that drew failing with an error): back to it
        note_in(&dir, "", 0, Stage::Trying);
        assert_eq!(start_in(&dir, 2), 1);
        // nothing left beside it
        let files: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(Result::ok).collect();
        assert_eq!(files.len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_backend_that_drew_is_tried_again_once_the_app_or_the_system_is_updated() {
        let dir = std::env::temp_dir().join(format!("procraft-gpu-record-build-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::fs::remove_file(dir.join(RECORD));
        let before = "100 brand/tablet:15/A/1:user/release-keys";
        let after = "200 brand/tablet:15/A/1:user/release-keys";
        let later = "200 brand/tablet:16/B/2:user/release-keys";
        // the first backend draws
        note_in(&dir, before, 0, Stage::Trying);
        note_in(&dir, before, 0, Stage::Made);
        note_in(&dir, before, 0, Stage::Drew);
        assert_eq!(std::fs::read_to_string(dir.join(RECORD)).unwrap(), format!("0 drew\n{before}"));
        // in the same build a crash making its device later doesn't count against it
        note_in(&dir, before, 0, Stage::Trying);
        assert_eq!(start_in(&dir, 2), 0);
        // updated, its try counts again: a crash in it (new shaders, a new driver) sends the next
        // start to the next backend, not round to the same crash
        note_in(&dir, after, 0, Stage::Trying);
        assert_eq!(std::fs::read_to_string(dir.join(RECORD)).unwrap(), format!("0 trying\n{after}"));
        assert_eq!(start_in(&dir, 2), 1);
        // that one draws, and is kept while the build stays
        note_in(&dir, after, 1, Stage::Trying);
        note_in(&dir, after, 1, Stage::Made);
        note_in(&dir, after, 1, Stage::Drew);
        note_in(&dir, after, 1, Stage::Trying);
        assert_eq!(start_in(&dir, 2), 1);
        // the system updated, without a crash: the same backend, drawing as before
        note_in(&dir, later, 1, Stage::Trying);
        note_in(&dir, later, 1, Stage::Made);
        assert_eq!(start_in(&dir, 2), 1);
        // a record from before builds were kept (one line) is another build's
        std::fs::write(dir.join(RECORD), "0 drew").unwrap();
        assert_eq!(built_by("0 drew"), "");
        note_in(&dir, after, 0, Stage::Trying);
        assert_eq!(start_in(&dir, 2), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
