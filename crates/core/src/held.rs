//! Who holds a lock, and since when. A call that waits on one a long time (the app's own thread
//! waiting on the canvas would freeze the interface) says in the log what held it, on which thread
//! and for how long, so a log shared after a freeze names its cause.

use std::panic::Location;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A wait on a lock longer than this goes in the log, with what held it.
pub(crate) const LONG_WAIT: Duration = Duration::from_secs(1);

/// Where a lock was last taken, on which thread, and when.
#[derive(Default)]
pub(crate) struct Holder {
    last: parking_lot::Mutex<Option<Taken>>,
}

#[derive(Clone)]
struct Taken {
    at: &'static Location<'static>,
    thread: Arc<str>,
    since: Instant,
}

impl Holder {
    /// The lock was just taken at `at`, on this thread.
    pub(crate) fn took(&self, at: &'static Location<'static>) {
        let thread = THREAD.with(Arc::clone);
        *self.last.lock() = Some(Taken { at, thread, since: Instant::now() });
    }

    /// Who took the lock last, where and how long ago, as the log says it.
    pub(crate) fn said(&self) -> String {
        match self.last.lock().clone() {
            Some(t) => format!("taken at {} on {} {} ms before", place(t.at), t.thread, t.since.elapsed().as_millis()),
            None => "not taken yet".to_owned(),
        }
    }
}

/// `at` as the log says it: the file in the crate and the line.
pub(crate) fn place(at: &Location<'_>) -> String {
    let file = at.file().rsplit(['/', '\\']).next().unwrap_or(at.file());
    format!("{file}:{}", at.line())
}

std::thread_local! {
    /// This thread's name, read once.
    static THREAD: Arc<str> = Arc::from(thread_name());
}

/// The calling thread's name, as the log says it: the system's name for it where there is one
/// (the app's own thread, which Rust didn't start, has no name of Rust's).
pub(crate) fn thread_name() -> String {
    #[cfg(any(target_os = "android", target_os = "linux"))]
    if let Ok(name) = std::fs::read_to_string("/proc/thread-self/comm")
        && !name.trim().is_empty()
    {
        return name.trim().to_owned();
    }
    let t = std::thread::current();
    t.name().map_or_else(|| format!("{:?}", t.id()), str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_holder_says_where_and_on_which_thread_the_lock_was_taken() {
        let h = Holder::default();
        assert_eq!(h.said(), "not taken yet");
        std::thread::Builder::new()
            .name("taker".into())
            .spawn({
                let h = std::sync::Arc::new(h);
                move || {
                    h.took(Location::caller());
                    let said = h.said();
                    assert!(said.starts_with("taken at held.rs:"), "{said}");
                    assert!(said.contains(" on taker "), "{said}");
                    assert!(said.ends_with(" ms before"), "{said}");
                }
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
