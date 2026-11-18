//! A tablet's pen through its driver's WinTab interface (`wintab32.dll`), for Windows with the
//! driver's Windows Ink turned off: the pen then reaches the window only as the mouse, without
//! pressure and in whole pixels, so its pressure, eraser end, lean and where it is (finer than a
//! pixel) are read from WinTab's packets instead and handed on as a mouse pen's (as X11 and a Mac
//! give theirs). While the system's own pointer messages carry the pen (Windows Ink on), WinTab's
//! packets are taken off its queue and left alone.
//!
//! Drivers don't all make the context they are asked for: its packets are read as the context
//! opened says they are laid out, its messages taken from the base it gives them, and a pressure
//! range the device doesn't give (or the pen presses past) is found from the pen's own presses.
//!
//! A driver gives packets only to the app in front (Wacom's: the one with the keyboard), and may
//! not take a context put on top while the pen presses, as it does when its press brings the
//! window to the front: the context is enabled and put on top as the window comes to the front,
//! enabled anew and put on top again once the pen is up if no packet has come by then, and
//! re-opened if a press still finds none.

use std::ffi::c_void;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HMODULE, HWND, LPARAM, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{ClientToScreen, ScreenToClient};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetForegroundWindow};
use windows::core::{s, w};

/// The base WinTab's messages are asked to start from (its default), and where some of them are
/// from it: a packet is waiting, a context is closing, a context moved in the overlap order, and
/// the pen came near or went.
const WT_DEFBASE: u32 = 0x7FF0;
const PACKET: u32 = 0;
const CTXCLOSE: u32 = 2;
const CTXOVERLAP: u32 = 4;
const PROXIMITY: u32 = 5;
/// The first message number left to apps: a base below it isn't one a context can use.
const WM_USER: u32 = 0x0400;
/// A context's status: disabled, and on top of the overlap order.
const CXS_DISABLED: u32 = 0x0001;
const CXS_ONTOP: u32 = 0x0004;

/// The base of the messages the open context sends (0: none is open).
static BASE: AtomicU32 = AtomicU32::new(0);
/// The open context's handle (0: none), for the messages about a context that name it.
static CONTEXT: AtomicUsize = AtomicUsize::new(0);

/// What [`WTInfoW`] is asked: the default digitizing and system contexts, a device's name,
/// pressure and orientation axes, and a cursor's name.
const WTI_DEFCONTEXT: u32 = 3;
const WTI_DEFSYSCTX: u32 = 4;
const WTI_DEVICES: u32 = 100;
const WTI_CURSORS: u32 = 200;
const DVC_NAME: u32 = 1;
const DVC_NPRESSURE: u32 = 15;
const DVC_ORIENTATION: u32 = 17;
const CSR_NAME: u32 = 1;
/// A context's options: it moves the system's cursor, and sends its messages to the window.
const CXO_SYSTEM: u32 = 0x0001;
const CXO_MESSAGES: u32 = 0x0004;
/// What a packet carries, a bit each, laid out in the order of the bits.
const PK_CONTEXT: u32 = 0x0001;
const PK_CURSOR: u32 = 0x0020;
const PK_BUTTONS: u32 = 0x0040;
const PK_X: u32 = 0x0080;
const PK_Y: u32 = 0x0100;
const PK_NORMAL_PRESSURE: u32 = 0x0400;
const PK_ORIENTATION: u32 = 0x1000;
const PK_ROTATION: u32 = 0x2000;
/// The fields WinTab defines, the bits up to [`PK_ROTATION`]: a driver's own come after them.
const PK_FIELDS: u32 = 14;
/// What the context is asked for: which end of which pen, the buttons, where it is, the pressure
/// and the orientation.
const PACKET_DATA: u32 = PK_CURSOR | PK_BUTTONS | PK_X | PK_Y | PK_NORMAL_PRESSURE | PK_ORIENTATION;
/// Room for a packet: every field WinTab defines (80 bytes), and what a driver's own add.
const PACKET_ROOM: usize = 512;
/// The most a name read from the driver may take, bytes.
const NAME_ROOM: usize = 256;
/// How far (pixels, each way) where a packet puts the pen may be from the mouse pointer the
/// driver moved for it, and still be taken: further, the tablet is mapped otherwise than its
/// context says, and the pointer's whole pixels are taken instead.
const FINE_REACH: f32 = 1.5;
/// How many packets the context's queue is asked to hold, the most first: the driver's own holds
/// 8, 40 ms of a pen read 200 times a second, and the packets that come while a slow frame is
/// drawn past that are lost (the stroke goes on at the pressure read before it). A size the driver
/// can't make leaves the context no queue, so smaller ones are tried until one is made.
const QUEUE_SIZES: [i32; 5] = [128, 64, 32, 16, 8];
/// The fewest pressure levels a range found from the pen's presses has: tablets give 1024 or more.
const LEAST_LEVELS: i64 = 1024;

/// WinTab's context description (its `LOGCONTEXTW`, 212 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
struct LogContext {
    name: [u16; 40],
    options: u32,
    status: u32,
    locks: u32,
    msg_base: u32,
    device: u32,
    pkt_rate: u32,
    pkt_data: u32,
    pkt_mode: u32,
    move_mask: u32,
    btn_dn_mask: u32,
    btn_up_mask: u32,
    in_org: [i32; 3],
    in_ext: [i32; 3],
    out_org: [i32; 3],
    out_ext: [i32; 3],
    sens: [u32; 3],
    sys_mode: i32,
    sys_org: [i32; 2],
    sys_ext: [i32; 2],
    sys_sens: [u32; 2],
}

/// A context description with nothing in it.
const BLANK: LogContext = LogContext {
    name: [0; 40],
    options: 0,
    status: 0,
    locks: 0,
    msg_base: 0,
    device: 0,
    pkt_rate: 0,
    pkt_data: 0,
    pkt_mode: 0,
    move_mask: 0,
    btn_dn_mask: 0,
    btn_up_mask: 0,
    in_org: [0; 3],
    in_ext: [0; 3],
    out_org: [0; 3],
    out_ext: [0; 3],
    sens: [0; 3],
    sys_mode: 0,
    sys_org: [0; 2],
    sys_ext: [0; 2],
    sys_sens: [0; 2],
};

/// One of a device's axes (its `AXIS`).
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Axis {
    min: i32,
    max: i32,
    units: u32,
    resolution: u32,
}

/// A packet's room, as aligned as a packet's own fields.
#[repr(C, align(8))]
struct Room([u8; PACKET_ROOM]);

/// What a context's packets carry (`data`, a bit each) and which of it comes as a change from the
/// packet before (`relative`), as the context opened has it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Layout {
    data: u32,
    relative: u32,
}

/// A packet's fields this app reads, each if the packet carries it: the cursor (which end of
/// which pen), the buttons held, where the pen is (the tablet's own units, from its top left), the
/// pressure, and the orientation (azimuth and altitude in tenths of a degree, twist).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Packet {
    cursor: Option<u32>,
    buttons: Option<u32>,
    x: Option<i32>,
    y: Option<i32>,
    pressure: Option<i32>,
    orientation: Option<[i32; 3]>,
}

impl Layout {
    /// The fields of `bytes`, a packet as WinTab lays one out: a field for each bit, in the order
    /// of the bits, the context's handle as wide as a pointer, an orientation and a rotation three
    /// numbers each, the rest one number (four bytes); a driver's own fields after them. None if
    /// `bytes` is too short for them.
    fn read(&self, bytes: &[u8]) -> Option<Packet> {
        let number = |at: usize| bytes.get(at..at + 4).and_then(|b| b.try_into().ok()).map(i32::from_ne_bytes);
        let mut out = Packet::default();
        let mut at = 0;
        for flag in (0..PK_FIELDS).map(|bit| 1u32 << bit).filter(|flag| self.data & flag != 0) {
            let size = match flag {
                PK_CONTEXT => std::mem::size_of::<usize>(),
                PK_ORIENTATION | PK_ROTATION => 12,
                _ => 4,
            };
            if bytes.len() < at + size {
                return None;
            }
            match flag {
                PK_CURSOR => out.cursor = number(at).map(|n| n as u32),
                PK_BUTTONS => out.buttons = number(at).map(|n| n as u32),
                PK_X => out.x = number(at),
                PK_Y => out.y = number(at),
                PK_NORMAL_PRESSURE => out.pressure = number(at),
                PK_ORIENTATION => out.orientation = Some([number(at)?, number(at + 4)?, number(at + 8)?]),
                _ => {}
            }
            at += size;
        }
        Some(out)
    }
}

/// Packets read in turn, for the fields that come as changes: the pressure and the buttons as
/// they are now.
#[derive(Clone, Copy, Debug)]
struct Reader {
    layout: Layout,
    pressure: i32,
    buttons: u32,
}

impl Reader {
    fn new(layout: Layout) -> Reader {
        Reader { layout, pressure: 0, buttons: 0 }
    }

    /// `p` with each field as it is, not as a change: a pressure's change added to what it was;
    /// a button's change (the low word its number, the high word 1 for up, 2 for down) made to the
    /// buttons held; and where the pen is left out, as a move says nothing of where it is.
    fn next(&mut self, p: Packet) -> Packet {
        let relative = |flag: u32| self.layout.relative & flag != 0;
        let mut out = p;
        if let Some(change) = p.pressure
            && relative(PK_NORMAL_PRESSURE)
        {
            self.pressure = self.pressure.saturating_add(change).max(0);
            out.pressure = Some(self.pressure);
        }
        if let Some(code) = p.buttons
            && relative(PK_BUTTONS)
        {
            let bit = 1u32.checked_shl(code & 0xFFFF).unwrap_or(0);
            match code >> 16 {
                1 => self.buttons &= !bit,
                2 => self.buttons |= bit,
                _ => {}
            }
            out.buttons = Some(self.buttons);
        }
        if relative(PK_X) || relative(PK_Y) {
            (out.x, out.y) = (None, None);
        }
        out
    }
}

/// The pressure's range: as the device gives it, unless it gives none (nothing wider than one
/// step), or the pen presses past it: the range is then taken to run to the next whole number of
/// levels (a power of two, as tablets' go: 1024, 2048 ... 8192), at least 1024.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pressures {
    low: i64,
    high: i64,
}

impl Pressures {
    fn of(axis: Option<(i32, i32)>) -> Pressures {
        match axis {
            Some((low, high)) if i64::from(high) > i64::from(low) + 1 => {
                Pressures { low: i64::from(low), high: i64::from(high) }
            }
            _ => Pressures { low: 0, high: LEAST_LEVELS - 1 },
        }
    }

    /// The pressure `raw` as 0..1 of the range, and the range's new top if it had to widen for it.
    fn take(&mut self, raw: i32) -> (f32, Option<i64>) {
        let raw = i64::from(raw);
        let widened = (raw > self.high).then(|| {
            let mut levels = LEAST_LEVELS;
            while levels - 1 < raw {
                levels *= 2;
            }
            self.high = levels - 1;
            self.high
        });
        let span = (self.high - self.low).max(1) as f64;
        (((raw - self.low) as f64 / span).clamp(0.0, 1.0) as f32, widened)
    }
}

/// Where packets put the pen on the screen: how far the tablet's units run across and down, and
/// the screen's area (pixels) the driver maps the tablet to, as its system context says.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Area {
    tablet: (f64, f64),
    screen_origin: (f64, f64),
    screen_size: (f64, f64),
}

impl Area {
    /// The area a system context `c` maps its tablet to (as the driver describes it, before the
    /// context is asked for packets in the tablet's own units); none if it says nothing of either.
    fn of(c: &LogContext) -> Option<Area> {
        let tablet = (f64::from(c.in_ext[0]).abs(), f64::from(c.in_ext[1]).abs());
        let screen_size = (f64::from(c.sys_ext[0]).abs(), f64::from(c.sys_ext[1]).abs());
        let none = |(a, b): (f64, f64)| a < 1.0 || b < 1.0;
        if none(tablet) || none(screen_size) {
            return None;
        }
        Some(Area { tablet, screen_origin: (f64::from(c.sys_org[0]), f64::from(c.sys_org[1])), screen_size })
    }

    /// Where on the screen (pixels, finer than whole ones) a packet's `x` and `y` put the pen.
    fn on_screen(&self, x: i32, y: i32) -> (f64, f64) {
        (
            self.screen_origin.0 + f64::from(x) / self.tablet.0 * self.screen_size.0,
            self.screen_origin.1 + f64::from(y) / self.tablet.1 * self.screen_size.1,
        )
    }
}

/// Where in the window (pixels) a packet puts the pen, from where it is `on_screen` and the
/// window's top left on the screen; none if that isn't within [`FINE_REACH`] of `pointer`, where
/// the driver put the mouse pointer for it.
fn fine_in_window(on_screen: (f64, f64), window: (i32, i32), pointer: (f32, f32)) -> Option<(f32, f32)> {
    let fine = ((on_screen.0 - f64::from(window.0)) as f32, (on_screen.1 - f64::from(window.1)) as f32);
    let near = (fine.0 - pointer.0).abs() <= FINE_REACH && (fine.1 - pointer.1).abs() <= FINE_REACH;
    near.then_some(fine)
}

type WtInfo = unsafe extern "system" fn(u32, u32, *mut c_void) -> u32;
type WtOpen = unsafe extern "system" fn(HWND, *mut LogContext, i32) -> *mut c_void;
type WtGet = unsafe extern "system" fn(*mut c_void, *mut LogContext) -> i32;
type WtPacket = unsafe extern "system" fn(*mut c_void, u32, *mut c_void) -> i32;
type WtOverlap = unsafe extern "system" fn(*mut c_void, i32) -> i32;
type WtEnable = unsafe extern "system" fn(*mut c_void, i32) -> i32;
type WtClose = unsafe extern "system" fn(*mut c_void) -> i32;
type WtQueueSizeSet = unsafe extern "system" fn(*mut c_void, i32) -> i32;

/// The driver's calls (those a driver may lack, if it does) and the context opened on the window,
/// with how its packets are read, the pressure's range, whether the device gives an orientation,
/// where its packets put the pen on the screen, and what the log has been told of them (the
/// cursors heard from, the pen's first press, packets without a pressure).
struct Tablet {
    info: WtInfo,
    packet: WtPacket,
    overlap: WtOverlap,
    enable: Option<WtEnable>,
    close: Option<WtClose>,
    get: Option<WtGet>,
    context: usize,
    reader: Reader,
    pressures: Pressures,
    leans: bool,
    area: Option<Area>,
    cursors_told: Vec<u32>,
    press_told: bool,
}

static TABLET: Mutex<Option<Tablet>> = Mutex::new(None);

/// When the system's pointer messages last carried a pen (ms since [`started`]): while lately,
/// Windows Ink has it and WinTab's packets are left alone.
static INK_AT: AtomicU64 = AtomicU64::new(0);
/// How long after the system's last pen message WinTab's are taken.
const INK_QUIET: Duration = Duration::from_millis(500);

fn started() -> Instant {
    static AT: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    *AT.get_or_init(Instant::now)
}

/// The system's pointer messages carry a pen now (Windows Ink is on for it).
pub fn ink_pen() {
    let quiet = !ink_lately();
    INK_AT.store((started().elapsed().as_millis() as u64).max(1), Ordering::Relaxed);
    // WinTab's first packets can come before the system's first message: what they said of the
    // pen isn't the mouse's, and would stay with it (its leaving is left alone while the system
    // has the pen)
    if quiet {
        super::set_mouse_pen(None);
        super::pen_flags(super::MOUSE, 0);
    }
}

fn ink_lately() -> bool {
    let at = INK_AT.load(Ordering::Relaxed);
    at != 0 && started().elapsed().as_millis() as u64 <= at + INK_QUIET.as_millis() as u64
}

/// A name the driver gives (`category`, `index`), unless it says it takes more than
/// [`NAME_ROOM`]: its size asked first, as some drivers write more than a fixed room holds.
fn name(info: WtInfo, category: u32, index: u32) -> String {
    // SAFETY: no room given: the driver only says how much the name takes
    if unsafe { info(category, index, std::ptr::null_mut()) } as usize > NAME_ROOM {
        return String::new();
    }
    let mut name = [0u16; NAME_ROOM / 2];
    // SAFETY: room for the name, in wide characters, as large as the driver said it takes
    unsafe { info(category, index, name.as_mut_ptr().cast()) };
    let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    String::from_utf16_lossy(name.get(..len).unwrap_or_default())
}

/// Opens a WinTab context on `hwnd` if a tablet's driver gives WinTab; nothing if none does.
pub fn open(hwnd: HWND) {
    // SAFETY: the driver's library, if there is one, loaded by its fixed name
    let Ok(library) = (unsafe { LoadLibraryW(w!("wintab32.dll")) }) else { return };
    let Some((info, open, packet, overlap)) = calls(library) else {
        log::warn!("WinTab: wintab32.dll lacks its calls");
        return;
    };
    let mut context = BLANK;
    // the system's context, which moves the pointer as the pen does; a driver without one, its
    // digitizing context made one
    // SAFETY: a context description to fill, as large as WinTab's
    let kind = if unsafe { info(WTI_DEFSYSCTX, 0, (&raw mut context).cast()) } != 0 {
        "system"
    } else if unsafe { info(WTI_DEFCONTEXT, 0, (&raw mut context).cast()) } != 0 {
        "digitizing"
    } else {
        log::warn!("WinTab: the driver gives no context");
        return;
    };
    context.options |= CXO_SYSTEM | CXO_MESSAGES;
    context.status = 0;
    // its messages from WinTab's own base, as this app takes them
    context.msg_base = WT_DEFBASE;
    // where the pen is, in the tablet's own units from its top left (finer than the screen's
    // pixels, which a system context gives), put on the screen as the driver maps the tablet
    let area = Area::of(&context);
    if area.is_some() {
        context.out_org = [0, 0, context.out_org[2]];
        context.out_ext = [context.in_ext[0].abs(), -context.in_ext[1].abs(), context.out_ext[2]];
    }
    context.pkt_data = PACKET_DATA;
    // every field as it is, not as a change from the last packet; packets whenever any changes
    context.pkt_mode = 0;
    context.move_mask = PACKET_DATA;
    // the axes of the device the context is on
    let device = WTI_DEVICES + context.device;
    let mut pressure = Axis::default();
    // SAFETY: an axis to fill
    let pressure = (unsafe { info(device, DVC_NPRESSURE, (&raw mut pressure).cast()) } != 0)
        .then_some((pressure.min, pressure.max));
    let mut orientation = [Axis::default(); 3];
    // SAFETY: room for the three axes it gives
    let leans = unsafe { info(device, DVC_ORIENTATION, orientation.as_mut_ptr().cast()) } != 0
        && orientation[1].max > orientation[1].min;
    // SAFETY: the window is the app's own; the description filled above
    let opened = unsafe { open(hwnd, &mut context, 1) };
    if opened.is_null() {
        log::warn!("WinTab: the context couldn't be opened");
        return;
    }
    // SAFETY: looked up by the name WinTab gives it, with the signature WinTab gives it
    let queue_size_set = unsafe { GetProcAddress(library, s!("WTQueueSizeSet")) }
        .map(|f| unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, WtQueueSizeSet>(f) });
    // SAFETY: the context just opened
    let queue = queue_size_set.and_then(|set| QUEUE_SIZES.into_iter().find(|&n| unsafe { set(opened, n) } != 0));
    if queue_size_set.is_some() && queue.is_none() {
        log::warn!("WinTab: no packet queue could be made");
    }
    // the context as the driver made it, which may not be as asked
    // SAFETY: looked up by the name WinTab gives it, with the signature WinTab gives it
    let get = unsafe { GetProcAddress(library, s!("WTGetW")) }
        .map(|f| unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, WtGet>(f) });
    let mut made = BLANK;
    // SAFETY: the context just opened, and a description to fill
    let made = get.is_some_and(|get| unsafe { get(opened, &mut made) } != 0).then_some(made);
    let (layout, base) = as_made(made.as_ref());
    let name = name(info, device, DVC_NAME);
    let range = pressure.map_or_else(|| "none".to_owned(), |(min, max)| format!("{min}..{max}"));
    log::info!(
        "WinTab: {name}, pressure {range}, {}, queue {}, {kind} context, packets {:#x}, changes {:#x}",
        if leans { "with lean" } else { "no lean" },
        queue.map_or_else(|| "as the driver's".to_owned(), |n| n.to_string()),
        layout.data,
        layout.relative
    );
    let pressures = Pressures::of(pressure);
    if pressure.is_none_or(|(min, max)| (pressures.low, pressures.high) != (i64::from(min), i64::from(max))) {
        log::info!("WinTab: the pressure taken as 0..{} until the pen presses past it", pressures.high);
    }
    if layout.data & PK_NORMAL_PRESSURE == 0 {
        log::warn!("WinTab: the driver's packets carry no pressure");
    }
    if base != WT_DEFBASE {
        log::info!("WinTab: messages from {base:#x}");
    }
    // SAFETY: each looked up by the name WinTab gives it, with the signature WinTab gives it
    let enable = unsafe { GetProcAddress(library, s!("WTEnable")) }
        .map(|f| unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, WtEnable>(f) });
    let close = unsafe { GetProcAddress(library, s!("WTClose")) }
        .map(|f| unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, WtClose>(f) });
    if let Ok(mut tablet) = TABLET.lock() {
        *tablet = Some(Tablet {
            info,
            packet,
            overlap,
            enable,
            close,
            get,
            context: opened as usize,
            reader: Reader::new(layout),
            pressures,
            leans,
            area,
            cursors_told: Vec::new(),
            press_told: false,
        });
        CONTEXT.store(opened as usize, Ordering::Relaxed);
        BASE.store(base, Ordering::Relaxed);
    }
}

/// How packets are laid out and the base the messages come from, as the driver `made` the context
/// (as asked, if it doesn't say): a context it says carries nothing, or a base apps can't use, is
/// taken as asked.
fn as_made(made: Option<&LogContext>) -> (Layout, u32) {
    let asked = Layout { data: PACKET_DATA, relative: 0 };
    let Some(made) = made else { return (asked, WT_DEFBASE) };
    let layout = if made.pkt_data == 0 { asked } else { Layout { data: made.pkt_data, relative: made.pkt_mode } };
    (layout, if made.msg_base >= WM_USER { made.msg_base } else { WT_DEFBASE })
}

/// The driver's four calls, by name.
fn calls(library: HMODULE) -> Option<(WtInfo, WtOpen, WtPacket, WtOverlap)> {
    // SAFETY: each is looked up by the name WinTab gives it and has the signature WinTab gives it
    unsafe {
        let info = GetProcAddress(library, s!("WTInfoW"))?;
        let open = GetProcAddress(library, s!("WTOpenW"))?;
        let packet = GetProcAddress(library, s!("WTPacket"))?;
        let overlap = GetProcAddress(library, s!("WTOverlap"))?;
        Some((
            std::mem::transmute::<unsafe extern "system" fn() -> isize, WtInfo>(info),
            std::mem::transmute::<unsafe extern "system" fn() -> isize, WtOpen>(open),
            std::mem::transmute::<unsafe extern "system" fn() -> isize, WtPacket>(packet),
            std::mem::transmute::<unsafe extern "system" fn() -> isize, WtOverlap>(overlap),
        ))
    }
}

/// What waits for the pen to be up (the mouse with no button held) while the window is in front.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Waiting {
    Nothing,
    /// The window came to the front: if no packet has come by then, the context is enabled anew
    /// and put on top again.
    Packets,
    /// Another context went over this one: it is put back on top.
    Top,
    /// The context is re-opened, for the reason given.
    Reopen(&'static str),
}

/// What is done to the context now.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Deed {
    Nothing,
    /// Enabled and put on top: the window came to the front.
    Front,
    /// Disabled and enabled again (its queue emptied) and put on top: no packet came.
    Anew,
    /// Put back on top.
    Top,
    Reopen(&'static str),
}

/// How long after the window comes to the front its context is left to give packets before it is
/// enabled anew for want of them: a driver's first may come a moment after the mouse's first move.
const FIRST_PACKETS: Duration = Duration::from_millis(200);

/// The context as the window comes to the front and goes back: what waits for the pen to be up,
/// when the window came to the front, whether a packet has come since, whether WinTab has given
/// any this run (a tablet's driver gives it), and what has been done since the window came to the
/// front for want of packets (the first packet after it told in the log) or for another context
/// over it.
#[derive(Clone, Copy, Debug)]
struct Front {
    waiting: Waiting,
    since: Option<Instant>,
    heard: bool,
    any_heard: bool,
    anew: bool,
    reopened: bool,
    retaken: bool,
    mended: bool,
}

impl Front {
    const fn new() -> Front {
        Front {
            waiting: Waiting::Nothing,
            since: None,
            heard: false,
            any_heard: false,
            anew: false,
            reopened: false,
            retaken: false,
            mended: false,
        }
    }

    fn came_to_front(&mut self, now: Instant) -> Deed {
        *self = Front { waiting: Waiting::Packets, since: Some(now), any_heard: self.any_heard, ..Front::new() };
        Deed::Front
    }

    /// Another window came to the front: nothing more is done to the context until this one does
    /// again (a context put on top would take the pen from the app in front).
    fn went_back(&mut self) {
        self.waiting = Waiting::Nothing;
    }

    /// A packet came; true if it is the first since something was done for want of them.
    fn packet(&mut self) -> bool {
        self.heard = true;
        self.any_heard = true;
        if self.waiting != Waiting::Top {
            self.waiting = Waiting::Nothing;
        }
        std::mem::take(&mut self.mended)
    }

    /// The mouse's button went down: if no packet has come since the context was enabled anew,
    /// with the pen up, it is re-opened once the pen is up again.
    fn pressed(&mut self) {
        if !self.heard && self.anew && !self.reopened && self.waiting == Waiting::Nothing {
            self.waiting = Waiting::Reopen("still no packets after it was enabled anew");
        }
    }

    /// The context moved in the overlap order, now `on_top` (and enabled) or not, with this
    /// window in `front` or not: put back on top once, should another go over it while in front.
    fn overlapped(&mut self, on_top: bool, front: bool) {
        if front && !on_top && !self.retaken && self.waiting == Waiting::Nothing {
            self.retaken = true;
            self.waiting = Waiting::Top;
        }
    }

    /// The context is gone (the driver closed it, or it can't be enabled or put on top): it is
    /// re-opened once the pen is up, once a time the window is in front.
    fn lost(&mut self, why: &'static str) {
        if !self.reopened {
            self.waiting = Waiting::Reopen(why);
        }
    }

    /// The pen is up (the mouse with no button held) at `now`, the window in `front` or not.
    fn pen_up(&mut self, front: bool, now: Instant) -> Deed {
        let early = self.since.is_some_and(|since| now < since + FIRST_PACKETS);
        if !front || (early && self.waiting == Waiting::Packets) {
            return Deed::Nothing;
        }
        match std::mem::replace(&mut self.waiting, Waiting::Nothing) {
            Waiting::Packets if !self.heard && self.any_heard => {
                self.anew = true;
                self.mended = true;
                Deed::Anew
            }
            Waiting::Nothing | Waiting::Packets => Deed::Nothing,
            Waiting::Top => Deed::Top,
            Waiting::Reopen(why) => {
                self.reopened = true;
                self.mended = true;
                Deed::Reopen(why)
            }
        }
    }
}

static FRONT: Mutex<Front> = Mutex::new(Front::new());

fn front<T>(f: impl FnOnce(&mut Front) -> T) -> Option<T> {
    FRONT.lock().ok().map(|mut front| f(&mut front))
}

/// The window came to the front (`active`) or went back. Coming to the front, its context is
/// enabled and put on top of the others (as WinTab asks; called once the window has the keyboard,
/// as a driver may give packets only to the app that has it). Going back, what WinTab said of the
/// pen is let go: the driver gives the window nothing more of it, not even its leaving, and a
/// press brought back would take the pressure it last read.
pub fn activated(hwnd: HWND, active: bool) {
    if active {
        if let Some(deed) = front(|f| f.came_to_front(Instant::now())) {
            act(hwnd, deed);
        }
    } else {
        front(Front::went_back);
        super::set_mouse_pen(None);
        super::pen_flags(super::MOUSE, 0);
    }
}

/// The mouse's left button went down (a tablet's pen moving it pressing, perhaps).
pub fn pressed() {
    front(Front::pressed);
}

/// The mouse moved, or a button came up, with none held: the pen, if it moves the mouse, is up.
pub fn pen_up(hwnd: HWND) {
    // SAFETY: no arguments; the window the user works in
    let in_front = unsafe { GetForegroundWindow() } == hwnd;
    pen_up_in_front(hwnd, in_front, Instant::now());
}

fn pen_up_in_front(hwnd: HWND, in_front: bool, now: Instant) {
    if let Some(deed) = front(|f| f.pen_up(in_front, now)) {
        act(hwnd, deed);
    }
}

/// A context's status in words, for the log.
fn standing(status: u32) -> &'static str {
    if status & CXS_DISABLED != 0 {
        "disabled"
    } else if status & CXS_ONTOP != 0 {
        "on top"
    } else {
        "under another"
    }
}

/// Does `deed` to the context, with a line in the log.
fn act(hwnd: HWND, deed: Deed) {
    let why = match deed {
        Deed::Nothing => return,
        Deed::Reopen(why) => return reopen(hwnd, why),
        Deed::Front => "the window came to the front: the context enabled and put on top",
        Deed::Anew => {
            "no packets since the window came to the front: the context enabled anew and put on top, the pen up"
        }
        Deed::Top => "another context went over this one: put back on top",
    };
    // the calls taken out first: the driver tells the window of the context's moves as they are
    // made, and those messages come back here
    let Some((enable, overlap, get, context)) =
        TABLET.lock().ok().and_then(|t| t.as_ref().map(|t| (t.enable, t.overlap, t.get, t.context)))
    else {
        return;
    };
    let context = context as *mut c_void;
    let mut was = BLANK;
    // SAFETY: the context this app opened, and a description to fill
    let was = get.filter(|&get| unsafe { get(context, &mut was) } != 0).map(|_| standing(was.status));
    // SAFETY: the context this app opened (each call fails, and does nothing, if it is gone)
    let enabled = enable.is_none_or(|enable| unsafe {
        if deed == Deed::Anew {
            enable(context, 0);
        }
        enable(context, 1) != 0
    });
    // SAFETY: as above
    let on_top = unsafe { overlap(context, 1) } != 0;
    log::info!("WinTab: {why}{}", was.map_or_else(String::new, |was| format!(" (it was {was})")));
    if !enabled || !on_top {
        log::warn!("WinTab: the context couldn't be {}", if enabled { "put on top" } else { "enabled" });
        front(|f| f.lost("it couldn't be enabled or put on top"));
    }
}

/// The context closed and opened again on `hwnd`, for the reason `why`: what was learnt of the
/// pen (the pressure's range found from its presses, what the log was told) kept.
fn reopen(hwnd: HWND, why: &str) {
    log::info!("WinTab: the context re-opened: {why}");
    BASE.store(0, Ordering::Relaxed);
    CONTEXT.store(0, Ordering::Relaxed);
    let old = TABLET.lock().ok().and_then(|mut t| t.take());
    if let Some(old) = &old
        && let Some(close) = old.close
    {
        // SAFETY: the context this app opened (closing one already gone fails, and does nothing);
        // its messages left alone, none being open now
        unsafe { close(old.context as *mut c_void) };
    }
    open(hwnd);
    if let (Some(old), Ok(mut tablet)) = (old, TABLET.lock())
        && let Some(new) = tablet.as_mut()
    {
        if new.pressures.low == old.pressures.low && new.pressures.high < old.pressures.high {
            new.pressures = old.pressures;
        }
        new.cursors_told = old.cursors_told;
        new.press_told = old.press_told;
    }
}

/// A WinTab message to the window: a packet read and handed on as the mouse pen's, the pen gone,
/// or this app's context closing or moving in the overlap order. True if it was one.
pub fn message(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> bool {
    let base = BASE.load(Ordering::Relaxed);
    if base == 0 {
        return false;
    }
    match msg.wrapping_sub(base) {
        PACKET => {
            let Ok(mut tablet) = TABLET.lock() else { return true };
            let Some(t) = tablet.as_mut() else { return true };
            let mut room = Room([0; PACKET_ROOM]);
            // SAFETY: the packet's serial number from the message, read into room for any packet;
            // it and those before it leave the context's queue
            let read = unsafe { (t.packet)(t.context as *mut c_void, wparam.0 as u32, room.0.as_mut_ptr().cast()) };
            // (read in turn even while Windows Ink has the pen: left on the queue, the packets
            // would fill it, and a change in one is a change from the one before)
            let Some(p) = (read != 0).then(|| t.layout().read(&room.0)).flatten().map(|p| t.reader.next(p)) else {
                return true;
            };
            if front(Front::packet) == Some(true) {
                log::info!("WinTab: packets come again");
            }
            if ink_lately() {
                return true;
            }
            let pressure = t.pressure(&p);
            let pen = reading(&p, pressure, t.leans);
            // its side switches as a pen's first and second buttons (Gesture controls' pen buttons)
            super::pen_flags(super::MOUSE, button_flags(p.buttons.unwrap_or(0)));
            let mut at = POINT::default();
            // SAFETY: a point to fill, then made the window's own
            let placed = unsafe { GetCursorPos(&mut at).is_ok() && ScreenToClient(hwnd, &mut at).as_bool() };
            super::set_mouse_pen(Some(pen));
            if placed {
                let pointer = (at.x as f32, at.y as f32);
                // the window's top left on the screen, for where in it the packet puts the pen
                let mut window = POINT::default();
                // SAFETY: a point to fill, the window's own
                let fine = (unsafe { ClientToScreen(hwnd, &mut window) }.as_bool())
                    .then_some(t.area.zip(p.x.zip(p.y)))
                    .flatten()
                    .and_then(|(a, (x, y))| fine_in_window(a.on_screen(x, y), (window.x, window.y), pointer));
                super::keep_fine_mouse_pen_at(pointer, pen.0, fine);
            }
            true
        }
        PROXIMITY => {
            // the low word: the pen came into the tablet's reach (non-zero), or left it
            if lparam.0 & 0xFFFF == 0 && !ink_lately() {
                super::set_mouse_pen(None);
            }
            true
        }
        // the driver closing this app's context (a context this app closes itself isn't open)
        CTXCLOSE => {
            if wparam.0 == CONTEXT.load(Ordering::Relaxed) {
                log::warn!("WinTab: the driver closed the context");
                front(|f| f.lost("the driver closed it"));
            }
            true
        }
        // the context moved in the overlap order: its status in `lparam`
        CTXOVERLAP => {
            if wparam.0 == CONTEXT.load(Ordering::Relaxed) {
                let status = lparam.0 as u32;
                let on_top = status & CXS_ONTOP != 0 && status & CXS_DISABLED == 0;
                // SAFETY: no arguments
                let in_front = unsafe { GetForegroundWindow() } == hwnd;
                front(|f| f.overlapped(on_top, in_front));
            }
            true
        }
        _ => false,
    }
}

impl Tablet {
    fn layout(&self) -> Layout {
        self.reader.layout
    }

    /// Packet `p`'s pressure, 0..1 (full if it carries none, as the mouse's); and what the log
    /// hasn't been told of it: a cursor heard from for the first time, the pen's first press, the
    /// range widened.
    fn pressure(&mut self, p: &Packet) -> f32 {
        if let Some(cursor) = p.cursor
            && !self.cursors_told.contains(&cursor)
        {
            self.cursors_told.push(cursor);
            let name = name(self.info, WTI_CURSORS + cursor, CSR_NAME);
            log::info!("WinTab: cursor {cursor} ({name}){}", if cursor % 3 == 2 { ", an eraser" } else { "" });
        }
        let pressed = p.buttons.unwrap_or(0) & 1 != 0 || p.pressure.unwrap_or(0) > 0;
        if pressed && !self.press_told {
            self.press_told = true;
            let (low, high) = (self.pressures.low, self.pressures.high);
            log::info!(
                "WinTab: the pen's first press: pressure {:?} of {low}..{high}, buttons {:?}",
                p.pressure,
                p.buttons
            );
        }
        let Some(raw) = p.pressure else { return 1.0 };
        let (pressure, widened) = self.pressures.take(raw);
        if let Some(high) = widened {
            log::info!("WinTab: the pen pressed to {raw}: the pressure taken as 0..{high}");
        }
        pressure
    }
}

/// A packet's buttons (a bit each: the tip, then the side switches) as a pen's flags: the lower
/// switch its first button, the upper its second.
fn button_flags(buttons: u32) -> u32 {
    let mut flags = 0;
    if buttons & 0b010 != 0 {
        flags |= super::FLAG_BARREL;
    }
    if buttons & 0b100 != 0 {
        flags |= super::FLAG_SECOND_BARREL;
    }
    flags
}

/// What a packet says, with its `pressure` (0..1): whether it is the eraser end, and its lean (from
/// upright, and which way, clockwise from the tablet's top), if its device `leans` (gives an
/// orientation) and the packet carries it.
fn reading(p: &Packet, pressure: f32, leans: bool) -> (super::MousePen, Option<(f32, f32)>) {
    // a tablet's cursors come in threes (puck, pen, eraser): the third of each is an eraser end
    let eraser = p.cursor.is_some_and(|c| c % 3 == 2);
    let lean = p.orientation.filter(|_| leans).map(|[azimuth, altitude, _]| {
        if altitude > 0 {
            let tilt = ((900 - altitude.clamp(0, 900)) as f32 / 10.0).to_radians();
            let way = (azimuth.rem_euclid(3600) as f32 / 10.0).to_radians();
            (tilt, way)
        } else {
            (0.0, 0.0)
        }
    });
    (super::MousePen { pressure, eraser }, lean)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A packet's bytes: `numbers`, each four, after a context's handle if `handle`.
    fn bytes(handle: bool, numbers: &[i32]) -> Vec<u8> {
        let mut out = if handle { vec![0xAB; std::mem::size_of::<usize>()] } else { Vec::new() };
        out.extend(numbers.iter().flat_map(|n| n.to_ne_bytes()));
        out
    }

    #[test]
    fn the_context_and_axis_are_laid_out_as_wintab_has_them() {
        assert_eq!(std::mem::size_of::<LogContext>(), 212);
        assert_eq!(std::mem::size_of::<Axis>(), 16);
    }

    #[test]
    fn a_packet_is_read_as_the_context_asked_lays_it_out() {
        let asked = Layout { data: PACKET_DATA, relative: 0 };
        // cursor, buttons, x, y, pressure, azimuth, altitude, twist
        let p = asked.read(&bytes(false, &[1, 0b011, 7600, 4750, 4095, 900, 450, 0])).unwrap();
        assert_eq!(
            p,
            Packet {
                cursor: Some(1),
                buttons: Some(0b011),
                x: Some(7600),
                y: Some(4750),
                pressure: Some(4095),
                orientation: Some([900, 450, 0]),
            }
        );
        // a packet cut short reads as none
        assert_eq!(asked.read(&bytes(false, &[1, 0, 7600, 4750, 4095, 900, 450])), None);
    }

    #[test]
    fn a_packet_laid_out_otherwise_than_asked_is_read_as_the_driver_made_it() {
        // a driver that gives no cursor or orientation but adds the status and the time: the
        // pressure is the fifth number, not the third
        let made = Layout { data: 0x0002 | 0x0004 | PK_BUTTONS | PK_X | PK_Y | PK_NORMAL_PRESSURE, relative: 0 };
        let p = made.read(&bytes(false, &[0, 123_456, 1, 100, 200, 2048])).unwrap();
        assert_eq!(
            (p.cursor, p.buttons, p.x, p.y, p.pressure, p.orientation),
            (None, Some(1), Some(100), Some(200), Some(2048), None)
        );
        // one that puts the context's handle first (as wide as a pointer), and the Z axis and
        // the tangent pressure among the rest
        let made =
            Layout { data: PK_CONTEXT | PK_CURSOR | PK_X | PK_Y | 0x0200 | PK_NORMAL_PRESSURE | 0x0800, relative: 0 };
        let p = made.read(&bytes(true, &[2, 10, 20, 0, 700, 0])).unwrap();
        assert_eq!((p.cursor, p.x, p.y, p.pressure), (Some(2), Some(10), Some(20), Some(700)));
        // one with no pressure in it
        let made = Layout { data: PK_CURSOR | PK_BUTTONS | PK_X | PK_Y, relative: 0 };
        assert_eq!(made.read(&bytes(false, &[1, 1, 5, 6])).unwrap().pressure, None);
        // a driver's own fields after WinTab's are left as they are
        let made = Layout { data: PK_NORMAL_PRESSURE | 0x8000, relative: 0 };
        assert_eq!(made.read(&bytes(false, &[300, 99])).unwrap().pressure, Some(300));
    }

    #[test]
    fn fields_that_come_as_changes_are_read_as_they_now_are() {
        let layout = Layout {
            data: PK_BUTTONS | PK_X | PK_Y | PK_NORMAL_PRESSURE,
            relative: PK_BUTTONS | PK_X | PK_NORMAL_PRESSURE,
        };
        let mut reader = Reader::new(layout);
        let packet = |buttons: u32, pressure: i32| Packet {
            buttons: Some(buttons),
            x: Some(3),
            y: Some(4),
            pressure: Some(pressure),
            ..Packet::default()
        };
        // the tip goes down (button 0, down) pressing 300, then 200 harder, then the lower
        // switch goes down too, then the tip comes up as the pressure goes
        let read = reader.next(packet(2 << 16, 300));
        assert_eq!(
            (read.buttons, read.pressure, read.x, read.y),
            (Some(0b1), Some(300), None, None),
            "where it is comes as a move"
        );
        assert_eq!(reader.next(packet(0, 200)).pressure, Some(500));
        assert_eq!(reader.next(packet(2 << 16 | 1, 0)).buttons, Some(0b11));
        let read = reader.next(packet(1 << 16, -600));
        assert_eq!((read.buttons, read.pressure), (Some(0b10), Some(0)), "never under none");
        // whole fields are as they come
        let mut whole = Reader::new(Layout { data: layout.data, relative: 0 });
        assert_eq!(whole.next(packet(0b1, 300)), packet(0b1, 300));
    }

    #[test]
    fn the_pressure_takes_the_devices_range_or_finds_one_from_the_pens_presses() {
        let read = |(got, widened): (f32, Option<i64>), want: f32, wider: Option<i64>| {
            (got - want).abs() < 1e-6 && widened == wider
        };
        let near = |got, want| read(got, want, None);
        // as the device gives it
        let mut range = Pressures::of(Some((0, 8191)));
        assert!(near(range.take(4096), 4096.0 / 8191.0));
        assert_eq!(range.take(8191), (1.0, None));
        // a device that gives none, or none wider than a step: 1024 levels until the pen presses
        // past them, then the next whole number of levels, the press read in those
        for none in [None, Some((0, 0)), Some((0, 1)), Some((5, 3))] {
            let mut range = Pressures::of(none);
            assert_eq!(range, Pressures { low: 0, high: 1023 }, "{none:?}");
            assert_eq!(range.take(1023), (1.0, None));
            assert!(read(range.take(1500), 1500.0 / 2047.0, Some(2047)));
            assert!(read(range.take(5000), 5000.0 / 8191.0, Some(8191)));
            assert!(near(range.take(4096), 4096.0 / 8191.0), "light presses are light from then on");
        }
        // a device that gives less than its pen presses (1024 levels, a pen of 8192)
        let mut range = Pressures::of(Some((0, 1023)));
        assert!(near(range.take(512), 512.0 / 1023.0));
        assert!(read(range.take(6000), 6000.0 / 8191.0, Some(8191)));
        assert!(near(range.take(512), 512.0 / 8191.0));
        // nothing pressed is none; a range not from none keeps its start
        assert_eq!(Pressures::of(Some((100, 1123))).take(50).0, 0.0);
        assert_eq!(range.take(-5).0, 0.0);
        assert_eq!(Pressures::of(None).take(i32::MAX), (1.0, Some(i64::from(i32::MAX))));
    }

    #[test]
    fn the_context_is_taken_as_the_driver_made_it() {
        let asked = Layout { data: PACKET_DATA, relative: 0 };
        assert_eq!(as_made(None), (asked, WT_DEFBASE), "a driver without WTGet: as asked");
        let made =
            LogContext { pkt_data: PK_BUTTONS | PK_NORMAL_PRESSURE, pkt_mode: PK_BUTTONS, msg_base: 0x6000, ..BLANK };
        assert_eq!(
            as_made(Some(&made)),
            (Layout { data: PK_BUTTONS | PK_NORMAL_PRESSURE, relative: PK_BUTTONS }, 0x6000)
        );
        // one that says its packets carry nothing, or gives a base apps can't use
        let made = LogContext { pkt_data: 0, msg_base: 0, ..BLANK };
        assert_eq!(as_made(Some(&made)), (asked, WT_DEFBASE));
    }

    #[test]
    fn a_packet_puts_the_pen_finer_than_a_pixel_where_the_driver_maps_the_tablet() {
        // a tablet 15200 by 9500 units mapped to a screen 1920 by 1200 right of another
        let context = LogContext { in_ext: [15200, 9500, 1023], sys_org: [1920, 0], sys_ext: [1920, 1200], ..BLANK };
        let area = Area::of(&context).unwrap();
        assert_eq!(area.on_screen(0, 0), (1920.0, 0.0));
        assert_eq!(area.on_screen(15200, 9500), (3840.0, 1200.0));
        // a unit is an eighth of a pixel across: the pen between pixels
        let (x, y) = area.on_screen(7601, 4750);
        assert!((x - 2880.126).abs() < 1e-3 && (y - 600.0).abs() < 1e-9, "{x} {y}");
        // in a window at (2000, 100), the driver having put the pointer on the pixel there
        assert_eq!(fine_in_window((x, y), (2000, 100), (880.0, 500.0)), Some(((x - 2000.0) as f32, 500.0)));
        // the tablet mapped otherwise than its context says (the pointer far off): none
        assert_eq!(fine_in_window((x, y), (2000, 100), (870.0, 500.0)), None);
        assert_eq!(fine_in_window((x, y), (2000, 100), (880.0, 502.0)), None);
        // a context that says nothing of the tablet or the screen gives no area
        assert_eq!(Area::of(&LogContext { sys_ext: [0, 0], ..context }), None);
        assert_eq!(Area::of(&LogContext { in_ext: [0, 9500, 0], ..context }), None);
    }

    #[test]
    fn the_side_switches_are_the_pens_buttons() {
        assert_eq!(button_flags(0b001), 0, "the tip alone");
        assert_eq!(button_flags(0b011), super::super::FLAG_BARREL);
        assert_eq!(button_flags(0b101), super::super::FLAG_SECOND_BARREL);
        assert_eq!(button_flags(0b110), super::super::FLAG_BARREL | super::super::FLAG_SECOND_BARREL);
    }

    #[test]
    fn a_packet_reads_as_the_pens_end_and_lean() {
        let p = Packet { cursor: Some(1), pressure: Some(512), orientation: Some([0, 900, 0]), ..Packet::default() };
        let (pen, lean) = reading(&p, 0.5, true);
        assert!(pen.pressure == 0.5 && !pen.eraser);
        assert_eq!(lean, Some((0.0, 0.0)), "upright");
        // the eraser end, leaning 45 degrees to the right
        let p = Packet { cursor: Some(2), orientation: Some([900, 450, 0]), ..Packet::default() };
        let (pen, lean) = reading(&p, 1.0, true);
        let (tilt, way) = lean.unwrap_or_default();
        assert!(pen.eraser);
        assert!((tilt - 45f32.to_radians()).abs() < 1e-4 && (way - 90f32.to_radians()).abs() < 1e-4);
        // a device without orientation, or a packet without one: no lean known; no cursor: a pen
        let p = Packet { cursor: Some(4), orientation: Some([0, 300, 0]), ..Packet::default() };
        assert_eq!(reading(&p, 1.0, false).1, None);
        assert_eq!(reading(&Packet { cursor: Some(1), ..Packet::default() }, 1.0, true).1, None);
        assert!(!reading(&Packet::default(), 1.0, true).0.eraser);
    }

    /// A stand-in driver: the next packet's numbers, and how many packets were taken off its queue.
    static STAND_IN: Mutex<Vec<i32>> = Mutex::new(Vec::new());
    static TAKEN: AtomicU32 = AtomicU32::new(0);

    unsafe extern "system" fn stand_in_packet(_: *mut c_void, _: u32, out: *mut c_void) -> i32 {
        TAKEN.fetch_add(1, Ordering::Relaxed);
        let bytes: Vec<u8> = STAND_IN.lock().unwrap().iter().flat_map(|n| n.to_ne_bytes()).collect();
        // SAFETY: the room the app gives holds any packet
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), out.cast::<u8>(), bytes.len()) };
        1
    }

    unsafe extern "system" fn stand_in_info(_: u32, _: u32, _: *mut c_void) -> u32 {
        0
    }

    /// What the stand-in driver was asked to do to a context, in turn: (call, context, argument).
    static ASKED: Mutex<Vec<(&'static str, usize, i32)>> = Mutex::new(Vec::new());
    /// What the stand-in driver's calls give back (0: they fail), and the status it gives.
    static GIVES: AtomicU32 = AtomicU32::new(1);
    static STATUS: AtomicU32 = AtomicU32::new(CXS_ONTOP);

    fn asked(call: &'static str, context: *mut c_void, argument: i32) -> i32 {
        ASKED.lock().unwrap().push((call, context as usize, argument));
        GIVES.load(Ordering::Relaxed) as i32
    }

    unsafe extern "system" fn stand_in_overlap(context: *mut c_void, top: i32) -> i32 {
        asked("overlap", context, top)
    }

    unsafe extern "system" fn stand_in_enable(context: *mut c_void, on: i32) -> i32 {
        asked("enable", context, on)
    }

    unsafe extern "system" fn stand_in_close(context: *mut c_void) -> i32 {
        asked("close", context, 0)
    }

    unsafe extern "system" fn stand_in_get(_: *mut c_void, out: *mut LogContext) -> i32 {
        // SAFETY: the description the app gives to fill
        unsafe { (*out).status = STATUS.load(Ordering::Relaxed) };
        1
    }

    /// A stand-in driver's context (handle 1) whose packets are laid out as `layout`, with no
    /// pressure range, its messages from a base of its own (0x6000).
    fn stand_in(layout: Layout) {
        *TABLET.lock().unwrap() = Some(Tablet {
            info: stand_in_info,
            packet: stand_in_packet,
            overlap: stand_in_overlap,
            enable: Some(stand_in_enable),
            close: Some(stand_in_close),
            get: Some(stand_in_get),
            context: 1,
            reader: Reader::new(layout),
            pressures: Pressures::of(None),
            leans: false,
            area: None,
            cursors_told: Vec::new(),
            press_told: false,
        });
        CONTEXT.store(1, Ordering::Relaxed);
        BASE.store(0x6000, Ordering::Relaxed);
    }

    #[test]
    fn a_drivers_packets_give_the_mouse_pointer_the_pens_pressure_as_the_driver_made_its_context() {
        let _one = super::super::tests::mouse_pointer_tests();
        INK_AT.store(0, Ordering::Relaxed);
        // a driver that gives the buttons and the pressure alone, the pressure as changes, no
        // pressure range, and its messages from a base of its own
        let layout = Layout { data: PK_BUTTONS | PK_NORMAL_PRESSURE, relative: PK_NORMAL_PRESSURE };
        stand_in(layout);
        let send = |numbers: &[i32]| {
            *STAND_IN.lock().unwrap() = numbers.to_vec();
            message(HWND::default(), 0x6000, WPARAM(1), LPARAM(0))
        };
        let pressure = || super::super::mouse_pen().map(|p| p.pressure);
        let near = |got: Option<f32>, want: f32| got.is_some_and(|p| (p - want).abs() < 1e-6);
        assert!(send(&[1, 512]));
        assert!(near(pressure(), 512.0 / 1023.0), "{:?}", pressure());
        // pressed past 1024 levels: 4096 of them from then on
        assert!(send(&[1, 2000]));
        assert!(near(pressure(), 2512.0 / 4095.0), "{:?}", pressure());
        // WinTab's own base isn't this context's, nor is the message after the proximity's
        assert!(!message(HWND::default(), WT_DEFBASE, WPARAM(1), LPARAM(0)));
        assert!(!message(HWND::default(), 0x6000 + 6, WPARAM(1), LPARAM(0)));
        // while Windows Ink has the pen, its packets still leave the queue, and are left alone
        // but for their changes
        ink_pen();
        let taken = TAKEN.load(Ordering::Relaxed);
        assert!(send(&[1, 100]));
        assert_eq!((TAKEN.load(Ordering::Relaxed), super::super::mouse_pen()), (taken + 1, None));
        INK_AT.store(0, Ordering::Relaxed);
        assert!(send(&[1, -612]));
        assert!(near(pressure(), 2000.0 / 4095.0), "{:?}", pressure());
        // the pen gone from the tablet
        assert!(message(HWND::default(), 0x6000 + PROXIMITY, WPARAM(0), LPARAM(0)));
        assert_eq!(super::super::mouse_pen(), None);
        BASE.store(0, Ordering::Relaxed);
        *TABLET.lock().unwrap() = None;
        super::super::pen_flags(super::super::MOUSE, 0);
        assert!(!message(HWND::default(), 0x6000, WPARAM(1), LPARAM(0)), "no context open");
    }

    #[test]
    fn the_context_is_enabled_and_put_on_top_as_the_window_comes_to_the_front_and_again_with_the_pen_up() {
        let at = Instant::now();
        let later = at + FIRST_PACKETS;
        let mut front = Front::new();
        // a tablet's driver giving packets: the window comes to the front, the pen pressing as it
        // does (a click on the canvas), and the packets come
        front.packet();
        assert_eq!(front.came_to_front(at), Deed::Front);
        front.pressed();
        assert!(!front.packet(), "nothing was done for want of them");
        assert_eq!(front.pen_up(true, later), Deed::Nothing);
        // the window back in front and no packet by the time the pen is up (the driver didn't
        // take the context put on top while the pen pressed): enabled anew and put on top
        assert_eq!(front.came_to_front(at), Deed::Front);
        front.pressed();
        assert_eq!(front.pen_up(true, at), Deed::Nothing, "the driver's first packets given a moment");
        assert_eq!(front.pen_up(true, later), Deed::Anew);
        assert_eq!(front.pen_up(true, later), Deed::Nothing, "once");
        // then they come: the log is told, once
        assert!(front.packet());
        assert!(!front.packet());
        front.pressed();
        assert_eq!(front.pen_up(true, later), Deed::Nothing);
        // still none after that, at the next press: the context re-opened once the pen is up, once
        front.came_to_front(at);
        assert_eq!(front.pen_up(true, later), Deed::Anew);
        front.pressed();
        assert!(matches!(front.pen_up(true, later), Deed::Reopen(_)));
        front.pressed();
        assert_eq!(front.pen_up(true, later), Deed::Nothing, "once a time the window is in front");
        // another window in front: nothing is done till this one is again (a context put on top
        // would take the pen from it)
        front.came_to_front(at);
        front.went_back();
        front.pressed();
        assert_eq!(front.pen_up(true, later), Deed::Nothing);
        front.came_to_front(at);
        assert_eq!(front.pen_up(false, later), Deed::Nothing, "it went back before the pen was up");
        assert_eq!(front.pen_up(true, later), Deed::Anew);
        // a run whose WinTab has given no packet (a mouse; a pen display left to Windows Ink):
        // nothing more than putting it on top as the window comes to the front
        let mut none = Front::new();
        assert_eq!(none.came_to_front(at), Deed::Front);
        assert_eq!(none.pen_up(true, later), Deed::Nothing);
        none.pressed();
        assert_eq!(none.pen_up(true, later), Deed::Nothing);
    }

    #[test]
    fn the_context_goes_back_on_top_once_if_another_goes_over_it_and_is_re_opened_if_lost() {
        let at = Instant::now();
        let later = at + FIRST_PACKETS;
        let mut front = Front::new();
        front.packet();
        front.came_to_front(at);
        front.packet();
        // its own move to the top; another's over it with another window in front
        front.overlapped(true, true);
        front.overlapped(false, false);
        assert_eq!(front.pen_up(true, later), Deed::Nothing);
        // another's over it, or it disabled, with this window in front: put back on top, once
        front.overlapped(false, true);
        assert_eq!(front.pen_up(true, later), Deed::Top);
        front.overlapped(false, true);
        assert_eq!(front.pen_up(true, later), Deed::Nothing, "no fight with another app over the top");
        // the driver closing it: re-opened once the pen is up, with this window in front
        front.lost("closed");
        assert_eq!(front.pen_up(false, later), Deed::Nothing);
        assert_eq!(front.pen_up(true, later), Deed::Reopen("closed"));
        front.lost("closed");
        assert_eq!(front.pen_up(true, later), Deed::Nothing, "once a time the window is in front");
        front.came_to_front(at);
        front.lost("closed");
        assert_eq!(front.pen_up(true, later), Deed::Reopen("closed"));
        assert_eq!(standing(CXS_ONTOP), "on top");
        assert_eq!(standing(CXS_ONTOP | CXS_DISABLED), "disabled");
        assert_eq!(standing(0x0002), "under another");
    }

    #[test]
    fn the_window_coming_to_the_front_sees_to_the_drivers_context() {
        let _one = super::super::tests::mouse_pointer_tests();
        INK_AT.store(0, Ordering::Relaxed);
        *FRONT.lock().unwrap() = Front::new();
        let take = || std::mem::take(&mut *ASKED.lock().unwrap());
        take();
        stand_in(Layout { data: PK_BUTTONS | PK_NORMAL_PRESSURE, relative: 0 });
        let hwnd = HWND::default();
        let later = || Instant::now() + FIRST_PACKETS;
        let send = |numbers: &[i32]| {
            *STAND_IN.lock().unwrap() = numbers.to_vec();
            message(hwnd, 0x6000, WPARAM(1), LPARAM(0))
        };
        // the pen drawing, its packets coming; another window comes to the front: what WinTab
        // said of the pen is let go (the driver gives nothing more of it, nor its leaving)
        assert!(send(&[1, 700]));
        assert!(super::super::mouse_pen().is_some());
        activated(hwnd, false);
        assert_eq!(super::super::mouse_pen(), None);
        assert_eq!(take(), [], "nothing done to the context");
        // back in front: enabled and put on top; with packets coming, nothing more
        activated(hwnd, true);
        assert_eq!(take(), [("enable", 1, 1), ("overlap", 1, 1)]);
        assert!(send(&[1, 300]));
        pen_up_in_front(hwnd, true, later());
        assert_eq!(take(), []);
        // back in front by a press of the pen that the driver didn't take: none by the time it
        // is up, so enabled anew (its queue emptied) and put on top
        activated(hwnd, false);
        activated(hwnd, true);
        pressed();
        take();
        pen_up_in_front(hwnd, true, later());
        assert_eq!(take(), [("enable", 1, 0), ("enable", 1, 1), ("overlap", 1, 1)]);
        // still none at the next press: re-opened once it is up, the old context closed (none is
        // opened in its place here, there being no driver)
        pressed();
        pen_up_in_front(hwnd, true, later());
        assert_eq!(take().first(), Some(&("close", 1, 0)));
        assert!(TABLET.lock().unwrap().as_ref().is_none_or(|t| t.context != 1));
        // a driver whose calls fail (the context gone): re-opened once the pen is up
        stand_in(Layout { data: PK_BUTTONS | PK_NORMAL_PRESSURE, relative: 0 });
        GIVES.store(0, Ordering::Relaxed);
        activated(hwnd, true);
        assert_eq!(take(), [("enable", 1, 1), ("overlap", 1, 1)]);
        GIVES.store(1, Ordering::Relaxed);
        pen_up_in_front(hwnd, true, later());
        assert_eq!(take().first(), Some(&("close", 1, 0)));
        // the driver closing the context, and moving it, are the window's to hear
        stand_in(Layout { data: PK_BUTTONS | PK_NORMAL_PRESSURE, relative: 0 });
        assert!(message(hwnd, 0x6000 + CTXOVERLAP, WPARAM(1), LPARAM(CXS_ONTOP as isize)));
        assert!(message(hwnd, 0x6000 + CTXCLOSE, WPARAM(1), LPARAM(0)));
        BASE.store(0, Ordering::Relaxed);
        CONTEXT.store(0, Ordering::Relaxed);
        *TABLET.lock().unwrap() = None;
        *FRONT.lock().unwrap() = Front::new();
        super::super::set_mouse_pen(None);
        super::super::pen_flags(super::super::MOUSE, 0);
    }
}
