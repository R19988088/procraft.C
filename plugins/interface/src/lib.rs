//! Interface: the interface as one likes it, each part on while its switch is. Its size, a little
//! bigger or smaller for a screen it doesn't suit (and its parts each bigger or smaller again, if
//! one likes), and on a phone kept clear of the status bar and the camera; its accent, the blue of what is chosen and switched on, in a hue and saturation of
//! one's own; undo and redo without the notice naming what they took back; and panels left open
//! while one paints. It's a plugin, not part of the app: `tools/pack_plugin.py` makes a
//! `.procraft-plugin` file of it.
//!
//! The app asks how big to show, and each part on top of that, and draws at those numbers, whether
//! to keep clear (and lays the
//! interface out inside the system's bars and the screen's cutouts if so), which colour, and
//! whether to show the notice or close a panel. The drawing, undo and the panels stay the app's.
//! The interface is written down at the top of `crates/core/src/plugin_run.rs`.

#![cfg_attr(not(test), no_std)]

/// The controls by their place, as the manifest starts them.
static CONTROLS: kit::Controls<14> =
    kit::Controls::new([0.0, 1.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 212.0, 1.0, 0.0, 0.0]);
kit::controls!(CONTROLS);
const SIZE_ON: usize = 0;
const SIZE: usize = 1;
const PARTS: usize = 2;
/// The parts' sliders, in the order the app numbers the parts: floating panels, popovers and
/// menus, the side bar, the top bar, the panels.
const FIRST_PART: usize = 3;
const PART_COUNT: i32 = 5;
const CLEAR: usize = 8;
const COLOUR: usize = 9;
const HUE: usize = 10;
const SATURATION: usize = 11;
const HIDE_UNDO: usize = 12;
const PIN_PANELS: usize = 13;

/// The steps it moves in as a window gets smaller, percent, so a window dragged smaller doesn't lay
/// everything out again at each pixel.
const STEP: f32 = 5.0;

/// What a bigger size must leave the interface (points): the smallest window the desktop opens,
/// which every panel fits. A window with less shows at 100%.
const LEAST_WIDTH: f32 = 640.0;
const LEAST_HEIGHT: f32 = 480.0;

/// How bright the accent is: the usual blue's, 253 of 255.
const BRIGHTNESS: f32 = 253.0 / 255.0;

/// As chosen, but a bigger size only as far as it leaves the interface the room it needs, on a step.
fn fitted(chosen: f32, width: f32, height: f32) -> f32 {
    let room = (width / LEAST_WIDTH).min(height / LEAST_HEIGHT) * 100.0;
    let steps = (room / STEP) as i32 as f32;
    chosen.min((steps * STEP / 100.0).max(1.0))
}

/// How big the interface shows in a window `width` × `height` points at 100%: as measured while
/// Interface size is off.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_interface_scale(width: f32, height: f32) -> f32 {
    if CONTROLS.on(SIZE_ON) { fitted(CONTROLS.get(SIZE), width, height) } else { 1.0 }
}

/// How big part `part` of the interface shows next to the rest of it: as its slider says while
/// Interface size sizes the parts separately; as the rest otherwise, and for a part it doesn't know.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_interface_part_scale(part: i32) -> f32 {
    if !(0..PART_COUNT).contains(&part) || !CONTROLS.on(SIZE_ON) || !CONTROLS.on(PARTS) {
        return 1.0;
    }
    CONTROLS.get(FIRST_PART + part as usize).clamp(0.5, 1.5)
}

// whether the interface keeps clear of the status bar and the screen's cutouts: Interface size's
// switch for it
kit::yes!(procraft_keep_clear, || CONTROLS.on(SIZE_ON) && CONTROLS.on(CLEAR));

/// The accent (role 0) as 0xRRGGBB while Interface colour is on; any other role, or with it off,
/// left as the app has it.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_interface_colour(role: i32) -> i32 {
    if role != 0 || !CONTROLS.on(COLOUR) {
        return -1;
    }
    let hue = CONTROLS.get(HUE).clamp(0.0, 360.0);
    let saturation = CONTROLS.get(SATURATION).clamp(0.0, 1.0);
    let [r, g, b] = rgb(hue, saturation, BRIGHTNESS);
    (channel(r) << 16) | (channel(g) << 8) | channel(b)
}

kit::yes!(procraft_hide_undo_notice, || CONTROLS.on(HIDE_UNDO));
kit::yes!(procraft_pin_panels, || CONTROLS.on(PIN_PANELS));

/// A colour by its hue (degrees), saturation and brightness (0 to 1), as red, green and blue.
fn rgb(hue: f32, saturation: f32, brightness: f32) -> [f32; 3] {
    let chroma = brightness * saturation;
    let sector = (hue % 360.0) / 60.0;
    let whole = sector as i32;
    let rising = chroma * (1.0 - abs((sector - (whole - whole % 2) as f32) - 1.0));
    let grey = brightness - chroma;
    let [r, g, b] = match whole {
        0 => [chroma, rising, 0.0],
        1 => [rising, chroma, 0.0],
        2 => [0.0, chroma, rising],
        3 => [0.0, rising, chroma],
        4 => [rising, 0.0, chroma],
        _ => [chroma, 0.0, rising],
    };
    [r + grey, g + grey, b + grey]
}

fn abs(v: f32) -> f32 {
    if v < 0.0 { -v } else { v }
}

/// 0 to 1 as 0 to 255, to the nearest.
fn channel(v: f32) -> i32 {
    let v = v * 255.0 + 0.5;
    (if v < 0.0 {
        0.0
    } else if v > 255.0 {
        255.0
    } else {
        v
    }) as i32
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_part_answers_only_while_its_switch_is_on_and_the_size_s_switch_keeps_clear() {
        let said = || {
            let scale = procraft_interface_scale(1440.0, 900.0);
            (
                scale,
                procraft_keep_clear(),
                procraft_interface_colour(0),
                procraft_hide_undo_notice(),
                procraft_pin_panels(),
            )
        };
        let parts = || (0..PART_COUNT).map(|part| procraft_interface_part_scale(part)).collect::<Vec<_>>();
        // all off: as measured, the app's own colour
        assert_eq!(said(), (1.0, 0, -1, 0, 0));
        assert_eq!(parts(), [1.0; 5]);
        // its sliders do nothing with their switch off
        procraft_control(SIZE as i32, 0.8);
        procraft_control(CLEAR as i32, 1.0);
        for (i, part) in [0.5, 0.75, 1.25, 1.5, 0.9].into_iter().enumerate() {
            procraft_control((FIRST_PART + i) as i32, part);
        }
        assert_eq!(said(), (1.0, 0, -1, 0, 0));
        assert_eq!(parts(), [1.0; 5]);
        procraft_control(SIZE_ON as i32, 1.0);
        assert_eq!(said(), (0.8, 1, -1, 0, 0));
        // each part as its slider says only while they're sized separately; a part it doesn't know as
        // the rest
        assert_eq!(parts(), [1.0; 5]);
        procraft_control(PARTS as i32, 1.0);
        assert_eq!(parts(), [0.5, 0.75, 1.25, 1.5, 0.9]);
        assert_eq!((procraft_interface_part_scale(-1), procraft_interface_part_scale(PART_COUNT)), (1.0, 1.0));
        assert_eq!(said().0, 0.8, "the whole as it was");
        procraft_control(CLEAR as i32, 0.0);
        assert_eq!(said().1, 0);
        // the accent starts as the usual blue, and goes by its sliders
        procraft_control(COLOUR as i32, 1.0);
        assert_eq!(procraft_interface_colour(0), 0x0076FD);
        assert_eq!(procraft_interface_colour(1), -1, "a role it doesn't know");
        procraft_control(HUE as i32, 0.0);
        assert_eq!(procraft_interface_colour(0), 0xFD0000);
        procraft_control(SATURATION as i32, 0.0);
        assert_eq!(procraft_interface_colour(0), 0xFDFDFD);
        procraft_control(HIDE_UNDO as i32, 1.0);
        procraft_control(PIN_PANELS as i32, 1.0);
        assert_eq!(said(), (0.8, 0, 0xFDFDFD, 1, 1));
        for switch in [SIZE_ON, COLOUR, HIDE_UNDO, PIN_PANELS] {
            procraft_control(switch as i32, 0.0);
        }
        assert_eq!(said(), (1.0, 0, -1, 0, 0));
        assert_eq!(parts(), [1.0; 5]);
        // a control it doesn't have changes nothing
        procraft_control(14, 1.0);
        assert_eq!(said(), (1.0, 0, -1, 0, 0));
    }

    #[test]
    fn a_bigger_size_goes_no_further_than_the_window_has_room() {
        // room to spare: as chosen
        assert_eq!(fitted(1.25, 1440.0, 900.0), 1.25);
        assert_eq!(fitted(1.25, 800.0, 600.0), 1.25);
        // the smallest window keeps the interface it always had
        assert_eq!(fitted(1.25, 640.0, 480.0), 1.0);
        // the shorter side decides, by steps
        assert_eq!(fitted(1.25, 1440.0, 540.0), 1.1);
        assert_eq!(fitted(1.25, 710.0, 900.0), 1.1);
        // a window smaller than that never goes below 100%
        assert_eq!(fitted(1.25, 500.0, 400.0), 1.0);
        // and a smaller size is as chosen, whatever the window
        assert_eq!(fitted(0.75, 500.0, 400.0), 0.75);
        // a window not known yet leaves it alone
        assert_eq!(fitted(0.9, 0.0, 0.0), 0.9);
        assert_eq!(fitted(1.25, f32::NAN, f32::NAN), 1.0);
    }
}
