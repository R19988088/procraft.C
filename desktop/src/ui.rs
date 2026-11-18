//! The interface over the canvas: the bars (see [`crate::chrome`]), the popovers hanging from
//! the top bar, and the keyboard's shortcuts.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use egui::{Color32, Key, Modifiers, Rect, Stroke, Ui, vec2};
use procraft_core::{
    Blocked, CanvasInfo, CanvasSession, GesturePurpose, KeyCombo, KeyShortcut, ModeButton, ModeSwitch, Modes,
    QuickAction, ToolKind, TouchCommand, Trigger, mode_switch, mode_switch_deselects,
};

use crate::actions::{Act, Actions};
use crate::alert::{Alert, Answer, Asking};
use crate::chrome::{BarButton, Lit, SideBar};
use crate::colour_drop::{ColourDrop, Touch};
use crate::colours::{Colours, hsv_to_rgb, of_hex, srgb, take_rgb, to_hex};
use crate::copy_paste::Choice;
use crate::files::Format;
use crate::input::Input;
use crate::layers::Layers;
use crate::library::Library;
use crate::look::{Look, argb, pc};
use crate::overlays::{Overlays, snapped_to};
use crate::system_clipboard::{Pasted, SystemClipboard};

const POPOVER_FILL: Color32 = Color32::from_rgb(0x28, 0x2a, 0x2d);
/// A tool button held this long (seconds), as a long press, takes the current brush.
const TOOL_HOLD_S: f64 = 0.4;
/// The pictures and the videos Insert a file and Insert a photo take (and a drop on the canvas).
const PICTURES: [&str; 8] = ["png", "jpg", "jpeg", "webp", "gif", "bmp", "tif", "tiff"];
const VIDEOS: [&str; 6] = ["mp4", "m4v", "mov", "wmv", "avi", "mkv"];

/// Whether `path` is named as a picture is.
fn is_picture(path: &std::path::Path) -> bool {
    let extension = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    PICTURES.contains(&extension.as_str())
}

/// Why layer or group `name`, which would hide a change, takes none, by what hides it (`by`).
fn hidden_message(name: &str, by: Option<procraft_core::HiddenBy>) -> String {
    use procraft_core::HiddenBy as H;
    let why = match by {
        Some(H::MaskLayer) => "its layer is hidden",
        Some(H::Group) => "its group is hidden",
        Some(H::Inside) => "a layer in it is hidden",
        Some(H::Solo) => "another layer is shown alone",
        Some(H::Itself) | None => "it is hidden",
    };
    format!("\u{201c}{name}\u{201d} can't be changed while {why}.")
}
/// A video's frames come in at the animation's frame rate, or this one; this many at most.
const VIDEO_FPS: u32 = 15;
const MOST_VIDEO_FRAMES: u32 = 999;

/// What the interface asks the app to do once the frame is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Back to the gallery, the artwork kept.
    Gallery,
    New,
    Open,
    Save,
    SaveAs,
    /// The artwork written as this format; a PDF keeps its pictures at this JPEG quality.
    Export(Format, u8),
    /// The file a plugin makes of the artwork's layers (by its place among those offered).
    PluginExport(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Popover {
    Actions,
    Adjustments,
    Brushes,
    Layers,
    Colour,
}

/// The background's colour picked in the colour panel: as the panel shows it (hue, saturation,
/// brightness), the colour last sent to the background (0xRRGGBB), and whether any was.
#[derive(Clone, Copy, Debug, PartialEq)]
struct BackgroundPick {
    hsv: [f32; 3],
    sent: u32,
    changed: bool,
}

impl BackgroundPick {
    /// Starts on the background's colour `rgb`, a grey taking the hue `hue`.
    fn new(rgb: u32, hue: f32) -> BackgroundPick {
        let mut hsv = [hue, 0.0, 0.0];
        take_rgb(&mut hsv, rgb);
        BackgroundPick { hsv, sent: rgb, changed: false }
    }

    /// What the panel shows now (`hsv`): the colour to send to the background while it is open, if
    /// it differs from the last one sent.
    fn follow(&mut self, hsv: [f32; 3]) -> Option<u32> {
        self.hsv = hsv;
        let rgb = to_hex(hsv_to_rgb(hsv));
        (rgb != self.sent).then(|| {
            self.sent = rgb;
            self.changed = true;
            rgb
        })
    }

    /// The colour to keep as one undo step as the panel closes: the last sent, if any was.
    fn kept(self) -> Option<u32> {
        self.changed.then_some(self.sent)
    }
}

pub struct Interface {
    pub actions: Vec<Action>,
    /// Where the canvas shows, clear of the bar (points).
    pub canvas_rect: Rect,
    /// A popover is open: a press on the canvas only closes it.
    pub popover_open: bool,
    /// Only a panel is open (none of its menus): a press on the canvas goes to the canvas, waiting
    /// to be a tap, which only closes the panel, or a drag, which closes it and paints.
    pub panel_lets_through: bool,
    /// The last press went to the canvas, which closes the panel in its own time.
    canvas_pressed: bool,
    /// The file the artwork was last saved to (or opened from, if it saves back there).
    pub path: Option<PathBuf>,
    /// The artwork's edits when it was last saved: anything else is unsaved.
    pub saved_edits: u64,
    /// Inserts being read, each on a thread of its own.
    /// Inserts being read, each what it came to and whether it is to be transformed.
    inserting: Vec<std::sync::mpsc::Receiver<(Inserted, bool)>>,
    /// A Save As being written: where and at which edits, or why it couldn't be.
    pub saving: Option<std::sync::mpsc::Receiver<Result<(std::path::PathBuf, u64), String>>>,
    open: Option<Popover>,
    /// The background's colour being picked, in the colour panel shown in the Layers panel's
    /// place.
    background_pick: Option<BackgroundPick>,
    pub(crate) colours: Colours,
    drop: ColourDrop,
    library: Library,
    overlays: Overlays,
    /// The interface is hidden (full screen).
    bars_hidden: bool,
    /// A layer copied or cut, to paste.
    clipboard: Option<procraft_core::ThumbnailImage>,
    /// The system's clipboard: what is copied here goes on it for other apps, and what they copy
    /// Paste takes once it is the newer.
    system_clipboard: SystemClipboard,
    /// Paste's button shows, the window in front (it is looked at again as that begins).
    paste_shown: bool,
    /// The Copy & Paste panel is up.
    copy_paste: bool,
    /// Layer Select's list: where (points) and the layers drawn there, top first.
    layer_choice: Option<(egui::Pos2, Vec<u64>)>,
    actions_panel: Actions,
    /// A plugin's panel's slider being moved: its row, and where it is.
    plugin_panel_held: Option<(usize, f32)>,
    /// A plugin's panel took Space as the key press it asked for: let go, it is no tap.
    space_to_panel: bool,
    layers: Layers,
    pub look: Look,
    side_bar: SideBar,
    /// Transform: its box over the canvas and its toolbar, while open.
    transform: crate::transform::Transform,
    /// Selection: its toolbar and what shows while selecting, while open.
    selection: crate::selection::Selection,
    /// The gesture settings (kept between runs), and their page while it is open.
    gestures: std::sync::Arc<procraft_core::GestureSettings>,
    gesture_controls: crate::gesture_controls::GestureControls,
    /// Canvas information, while it is open.
    canvas_info: Option<crate::canvas_info::CanvasInfo>,
    /// The reference window.
    reference: crate::reference::Reference,
    /// Crop and Resize.
    crop: crate::crop::Crop,
    /// The drawing guide's editor.
    guide_editor: crate::guide_editor::GuideEditor,
    /// The artwork's folder in the gallery.
    pub artwork_dir: Option<String>,
    quick_menu: crate::quick_menu::QuickMenu,
    /// A pen button set to the eyedropper is held: its loupe follows the pen until it is let go.
    pen_picking: bool,
    /// The pen's first and second buttons as held last frame: let go, one held to erase gives the
    /// eraser back.
    pen_held: [bool; 2],
    /// An open adjustment's bar, panel and tap menu; Gradient Map's library (kept between runs).
    adjustments: crate::adjustments::Adjustments,
    gradients: std::sync::Arc<procraft_core::Gradients>,
    styled: bool,
    /// The current layer turned a change away: why, and the alert saying so (offering to unlock or
    /// show it, or open Layers).
    blocked: Option<(Blocked, u64)>,
    blocked_alert: Alert,
    model_warning_alert: Alert,
    /// Share's PDF asks how well it keeps its pictures first (for this format).
    pdf_alert: Alert,
    pdf_for: Option<Format>,
    /// A plugin file's one screen, and the file while it asks.
    plugin_alert: Alert,
    plugin_file: Option<Vec<u8>>,
    /// An export under way, as its card shows it.
    pub export_card: crate::export_card::ExportCard,
    /// The brushes changed (the library closed, a slider let go, a notch saved): keep them.
    pub keep_brushes: bool,
    /// A tool button held long enough has acted (its tap, when let go, does nothing).
    tool_hold_acted: bool,
    brushes_were_open: bool,
    /// The Prefs tab's settings (kept between runs), whether the side bar shows, and the licences
    /// while they show.
    prefs: crate::prefs::Prefs,
    side_bar_shown: bool,
    licences: bool,
    /// The keyboard shortcuts' list, from Help or while Ctrl (⌘) is held.
    shortcut_list: crate::shortcuts::Shortcuts,
    /// Animation Assist's bar, while it is on, and sharing the layers as an animation.
    animation: crate::animation::AnimationBar,
    animated_export: Option<crate::animated_export::AnimatedExport>,
    /// The export screen just ended, kept a frame: its preview's textures were drawn in the frame
    /// it ended in, and a texture let go of is gone before that frame is.
    ended_export: Option<crate::animated_export::AnimatedExport>,
    /// The artwork was opened from the gallery's Share only to be shared as an animation: when
    /// that ends, the gallery comes back.
    exporting_from_gallery: bool,
    /// The artwork's time-lapse: recorded, replayed, made into a video.
    time_lapse: crate::timelapse::TimeLapseUi,
    /// The brush studio, while open (the app draws its pad).
    pub studio: crate::studio::Studio,
    /// Advanced settings' page is up.
    settings: bool,
    /// The single touch gestures companion, with Advanced settings' switch on.
    companion: crate::companion::Companion,
    /// How many onion skin frames there were before the QuickMenu last turned them off.
    onion_kept: u32,
    /// Recolor, while on, and the colour it last filled with.
    recolor_rgb: [f32; 3],
    /// The brush in use (its tool, set and place in it) and the one chosen before it with the
    /// same tool, for Previous brush.
    brush_now: Option<(ToolKind, u32, u32)>,
    brush_last: Option<(u32, u32)>,
    /// The artwork paints on a model; its Lighting Studio, while open.
    model: bool,
    lighting: crate::lighting::LightingStudio,
    /// The colour last named (Color Description Notifications) and when.
    named: Option<(u32, Instant)>,
    /// Text being added or edited.
    text: crate::text::TextEditor,
    /// A field had the keys as the last frame ended.
    typing: bool,
}

impl Default for Interface {
    fn default() -> Interface {
        Interface::with_prefs(crate::prefs::Prefs::kept())
    }
}

impl Interface {
    /// The interface as it starts, with `prefs` (the ones kept, as the app runs).
    fn with_prefs(prefs: crate::prefs::Prefs) -> Interface {
        Interface {
            actions: Vec::new(),
            canvas_rect: Rect::EVERYTHING,
            popover_open: false,
            panel_lets_through: false,
            canvas_pressed: false,
            path: None,
            saved_edits: 0,
            saving: None,
            inserting: Vec::new(),
            open: None,
            background_pick: None,
            colours: Colours::default(),
            drop: ColourDrop::default(),
            library: Library::default(),
            overlays: Overlays::default(),
            bars_hidden: false,
            clipboard: None,
            system_clipboard: SystemClipboard::new(is_picture),
            paste_shown: false,
            copy_paste: false,
            layer_choice: None,
            actions_panel: Actions::default(),
            plugin_panel_held: None,
            space_to_panel: false,
            layers: Layers::default(),
            look: Look::default(),
            side_bar: SideBar::default(),
            transform: crate::transform::Transform::default(),
            selection: crate::selection::Selection::default(),
            gestures: procraft_core::GestureSettings::new(crate::files::kept_gestures()),
            gesture_controls: crate::gesture_controls::GestureControls::default(),
            canvas_info: None,
            reference: crate::reference::Reference::default(),
            crop: crate::crop::Crop::default(),
            guide_editor: crate::guide_editor::GuideEditor::default(),
            artwork_dir: None,
            quick_menu: crate::quick_menu::QuickMenu::default(),
            pen_picking: false,
            pen_held: [false; 2],
            adjustments: crate::adjustments::Adjustments::default(),
            gradients: procraft_core::Gradients::open(
                crate::files::app_dir().join("gradients.json").to_string_lossy().into_owned(),
            ),
            styled: false,
            blocked: None,
            blocked_alert: Alert::default(),
            model_warning_alert: Alert::default(),
            pdf_alert: Alert::default(),
            pdf_for: None,
            plugin_alert: Alert::default(),
            plugin_file: None,
            export_card: crate::export_card::ExportCard::default(),
            keep_brushes: false,
            tool_hold_acted: false,
            brushes_were_open: false,
            prefs,
            side_bar_shown: true,
            licences: false,
            shortcut_list: Default::default(),
            animation: crate::animation::AnimationBar::default(),
            animated_export: None,
            ended_export: None,
            exporting_from_gallery: false,
            time_lapse: crate::timelapse::TimeLapseUi::default(),
            studio: crate::studio::Studio::default(),
            settings: false,
            named: None,
            model: false,
            lighting: crate::lighting::LightingStudio::default(),
            companion: crate::companion::Companion::default(),
            onion_kept: 1,
            recolor_rgb: [0.0; 3],
            brush_now: None,
            brush_last: None,
            text: crate::text::TextEditor::default(),
            typing: false,
        }
    }
}

impl Interface {
    /// A Save As written meanwhile: its file kept as where the artwork saves, or why it couldn't
    /// be told. True while one is still being written.
    pub fn saved_as(&mut self) -> bool {
        let Some(saving) = &self.saving else { return false };
        match saving.try_recv() {
            Err(std::sync::mpsc::TryRecvError::Empty) => return true,
            Ok(Ok((path, edits))) => {
                self.path = Some(path);
                self.saved_edits = edits;
            }
            Ok(Err(e)) => crate::files::tell("Can't save", &e),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {}
        }
        self.saving = None;
        false
    }

    /// `session` is shown now (at `scale` window pixels a point): it paints in the colour chosen.
    pub fn opened(&mut self, session: &Arc<CanvasSession>, scale: f32) {
        // each artwork its own Reference window, none if it never had one
        self.reference.restore(session);
        self.model = session.model_info().is_some();
        self.lighting.forget();
        // brush files given while no canvas was open
        self.library.add_brushes(session, crate::files::take_pending_brushes(), true);
        session.set_touch_layer_opacity(None);
        if self.model {
            crate::lighting::light_model(session);
            self.model_warnings(session);
        }
        self.layers.forget();
        self.background_pick = None;
        self.text.forget();
        crate::text::prepare_fonts();
        self.transform.on = false;
        self.selection.on = false;
        self.drop.end_filling();
        session.use_colour_history(self.colours.history.clone());
        session.set_touch_settings(self.gestures.touch_settings(self.prefs.rapid_undo_ms()));
        session.use_gradients(self.gradients.clone());
        self.prefs.apply(session);
        self.time_lapse.open(self.artwork_dir.as_deref(), session.edits());
        self.adjustments.menu_at = None;
        self.colours.send(session);
        self.place_canvas(session, scale);
    }

    /// The artwork is put away (to the gallery, for another, or with the window): brushes coming
    /// in from files are waited for, Recolor ends keeping what it filled, Layer Opacity ends, and
    /// the selection tool closes (Color Fill filling what is selected).
    pub fn closing(&mut self, session: &CanvasSession) {
        // a plugin's panel goes with the artwork it was open over
        self.prefs.plugins.close_panel();
        self.plugin_panel_held = None;
        self.reference.keep(session);
        self.end_background_pick(session);
        self.library.finish_importing(session);
        self.end_recolor(session);
        session.set_touch_layer_opacity(None);
        self.selection.close(session);
    }

    /// A new artwork just started: without time-lapse if that is turned off.
    pub fn started_new(&mut self) {
        if self.prefs.no_time_lapse {
            self.time_lapse.set_recording(false);
        }
    }

    /// The artwork just opened from the gallery's Share, to be shared as `format`: the gallery
    /// comes back once that ends.
    pub fn share_animated_from_gallery(&mut self, format: crate::animated_export::Animated) {
        self.animated_export = Some(crate::animated_export::AnimatedExport::new(format));
        self.exporting_from_gallery = true;
    }

    /// Whether the canvas is shown on a second display too.
    /// What an artwork dragged out of the gallery becomes (Advanced settings' Drag and Drop export).
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn drag_format(&self) -> crate::files::Format {
        crate::files::DRAG_FORMATS.get(self.prefs.drag_format).copied().unwrap_or(crate::files::Format::Procraft)
    }

    /// The plugins, which say how big the interface shows.
    pub fn plugins(&self) -> &procraft_core::Plugins {
        &self.prefs.plugins
    }

    /// The plugins, to keep: what opens a plugin's artwork holds them.
    pub fn plugins_shared(&self) -> Arc<procraft_core::Plugins> {
        Arc::clone(&self.prefs.plugins)
    }

    /// Tap Assistance is on (Advanced settings).
    pub fn tap_assistance(&self) -> bool {
        self.prefs.tap_assistance
    }

    pub fn project_canvas(&self) -> bool {
        self.prefs.project_canvas
    }

    /// Project canvas goes off (its window closed on the other display).
    pub fn stop_projecting(&mut self) {
        self.prefs.project_canvas = false;
        self.prefs.keep();
    }

    /// Fitting the canvas keeps it clear of the bar.
    pub fn place_canvas(&self, session: &CanvasSession, scale: f32) {
        // with Fit Canvas Inside Interface, clear of the top bar and the side bar (on its side), as
        // they show
        if !self.prefs.fit_inside {
            session.set_fit_insets(0.0, 0.0, 0.0, 0.0);
            return;
        }
        let part = |p: procraft_core::InterfacePart| self.prefs.plugins.interface_part_scale(p);
        let side_bar = pc::SIDE_BAR_WIDTH * part(procraft_core::InterfacePart::SideBar);
        let side = if self.side_bar_shown { (side_bar + crate::panel::POPOVER_GAP) * scale } else { 0.0 };
        let (left, right) = if self.prefs.right_handed { (0.0, side) } else { (side, 0.0) };
        let bar = pc::BAR_HEIGHT * part(procraft_core::InterfacePart::TopBar);
        session.set_fit_insets(left, bar * scale, right, 0.0);
    }

    /// The app's fonts and look, set the first time anything shows.
    pub fn style_once(&mut self, ctx: &egui::Context) {
        if !self.styled {
            style(ctx);
            self.styled = true;
        }
    }

    /// The interface for a frame. Escape closes what is open (each popover, panel, menu, alert and
    /// page sees to it, taking it with [`escape`]); one nothing took turns the canvas back upright,
    /// unless it only let go of a field (egui drops the field's keys as the frame begins).
    pub fn show(&mut self, ui: &mut Ui, session: &std::sync::Arc<CanvasSession>, input: &mut Input) {
        let typing = self.typing;
        self.show_frame(ui, session, input);
        let ctx = ui.ctx();
        if !typing && escape_untaken(ctx) && self.canvas_shown() && input.idle() {
            session.settle_upright();
        }
        self.typing = ctx.memory(|m| m.focused().is_some());
        input.keys_taken = self.keys_taken();
    }

    fn show_frame(&mut self, ui: &mut Ui, session: &std::sync::Arc<CanvasSession>, input: &mut Input) {
        let ctx = ui.ctx().clone();
        self.style_once(&ctx);
        // how big each part of the interface shows, as the plugins on say, before any of it draws
        crate::panel::set_part_scales(&ctx, &self.prefs.plugins);
        self.ended_export = None;
        self.canvas_pressed = std::mem::take(&mut input.canvas_pressed);
        // looked at again soon while an insert is read, or brushes made
        let brushes_coming = self.brushes_added(session);
        // the autosave told of when it fails (once until a save works again), the palettes too
        if let Some(why) = session.take_save_failure() {
            self.overlays.notice(procraft_core::save_failure_said(why), true);
        }
        // a crop or a smudge the GPU had no memory for
        if let Some(why) = session.take_failure() {
            self.overlays.notice(why, true);
        }
        if self.colours.palettes.take_failure().is_some() {
            self.overlays.notice("The palettes couldn't be kept.", true);
        }
        // a mask current paints with masks' own colour
        let on_mask = session.layers().iter().any(|l| l.current && l.kind == procraft_core::LayerType::Mask);
        self.colours.follow_mask(session, on_mask);
        let paste_shown = (self.open == Some(Popover::Actions) || self.copy_paste) && ctx.input(|i| i.focused);
        if paste_shown && !self.paste_shown {
            self.system_clipboard.offer();
        }
        self.paste_shown = paste_shown;
        let pasting = self.pasted(session);
        if self.inserted(session) || brushes_coming || pasting {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        // the pen's barrel buttons: what Gesture controls set Pen button and Pen second button to
        // do (the first, set to nothing, the eraser and back; set to erase, the eraser while it is
        // held), where the pointer is; one set to the eyedropper picks while it is held, where the
        // pen hovers, and lets go to take it; with the canvas out of sight they do nothing, then
        // or later: the tool would change unseen (and one held to erase is let go)
        let covered = self.studio.open || self.time_lapse.replaying();
        let presses = crate::pen::take_button_presses().map(|n| if covered { 0 } else { n });
        let held = crate::pen::buttons_held().map(|h| h && !covered);
        let purposes = [Trigger::PenDoubleTap, Trigger::PenSqueeze].map(|t| self.gestures.purpose_of(t));
        let (x, y) = input.cursor().unwrap_or_default();
        for (button, (&pressed, &is_held)) in presses.iter().zip(&held).enumerate() {
            if purposes.get(button).copied().flatten() == Some(GesturePurpose::Eyedropper) {
                if is_held || pressed > 0 {
                    self.overlays.pick(&ctx, session, x, y);
                    self.pen_picking = true;
                    ctx.request_repaint();
                }
            } else {
                for _ in 0..pressed {
                    input.commands.extend(session.touch_pen_button(button == 1, x, y));
                    self.library.tool_changed();
                }
            }
            // let go since the last frame (or pressed and let go within this one)
            let was_held = self.pen_held.get(button).copied().unwrap_or(false);
            if !is_held && (was_held || pressed > 0) && session.touch_pen_button_up(button == 1) {
                self.library.tool_changed();
            }
        }
        self.pen_held = held;
        let eyedropper_held = held.iter().zip(&purposes).any(|(h, p)| *h && *p == Some(GesturePurpose::Eyedropper));
        if self.pen_picking && !eyedropper_held {
            self.pen_picking = false;
            if let Some(rgb) = self.overlays.finish_pick() {
                self.colours.set_rgb(session, rgb);
            }
        }
        self.commands(&ctx, session, input);
        // the brush library closed: what was done in it is kept
        let brushes_open = self.open == Some(Popover::Brushes);
        self.keep_brushes |= self.brushes_were_open && !brushes_open;
        self.brushes_were_open = brushes_open;
        let info = session.info();
        // a snapped shape's handles sit at window points, and the view can move under them from a
        // zoom, a pan or a turn that comes in through the input layer rather than as a command
        if self.overlays.shape.is_some() {
            self.overlays.shape = session.quickshape_info();
        }
        self.follow_brush(session, info.tool);
        self.shortcuts(&ctx, session, &info);
        self.overlays.cursor_style = self.prefs.cursor_style;
        input.hover_shown = false;
        input.turns_model = self.model && !session.model_flat();
        session.set_touch_companion(match self.companion.mode {
            crate::companion::Mode::None => None,
            crate::companion::Mode::Zoom => Some(procraft_core::CompanionMode::Zoom),
            crate::companion::Mode::Move => Some(procraft_core::CompanionMode::Move),
        });
        self.recolor_frame(&ctx, session);
        if let Some(opacity) = session.touch_layer_opacity().filter(|_| !self.bars_hidden) {
            crate::adjustments::layer_opacity_bar(&ctx, opacity, crate::panel::bar_shown(&ctx));
        }
        let lit = Lit {
            actions: self.open == Some(Popover::Actions),
            transform: self.transform.on,
            // while the selection tool is open, or while something is selected
            selection: self.selection.on || session.has_selection(),
            adjustments: self.open == Some(Popover::Adjustments)
                || session.adjustment_shown().adjustment.is_some()
                || session.recolor_on()
                || session.touch_layer_opacity().is_some(),
            layers: self.open == Some(Popover::Layers),
            panel_open: matches!(self.open, Some(Popover::Layers | Popover::Colour)),
        };
        let colour = srgb(self.colours.rgb());
        self.time_lapse.record(&ctx, session, input.idle());
        // the brush studio over everything (the library stays open under it)
        if let Some(new) = self.library.studio.take() {
            // a brush a plugin's brush engine draws opens in that plugin's own studio
            match session.brush_studio_action().filter(|_| new.is_none()) {
                Some(key) => self.run_plugin_action(&ctx, session, key),
                None => {
                    self.studio.plugin_rows = crate::studio::plugin_rows(&self.prefs.plugins);
                    self.studio.begin(&ctx, session, new);
                }
            }
        }
        // a plugin changed a brush (its own studio's): read again and kept
        if session.take_brushes_changed() {
            self.library.library_changed();
        }
        if self.studio.open {
            self.canvas_rect = Rect::NOTHING;
            self.animation.reset(session);
            if let Some(crate::studio::Asked::Closed) = self.studio.show(&ctx, &mut self.look, session, &mut self.prefs)
            {
                self.library.library_changed();
                self.keep_brushes = true;
            }
            session.set_backdrops(Vec::new());
            self.popover_open = false;
            return;
        }
        // the time-lapse replayed over everything
        if self.time_lapse.replaying() {
            self.canvas_rect = Rect::NOTHING;
            self.animation.reset(session);
            if let Some(said) = self.time_lapse.show(&ctx, &mut self.look, &session.name()) {
                crate::files::tell("Time-lapse", said);
            }
            session.set_backdrops(Vec::new());
            self.popover_open = false;
            return;
        }
        // sharing as an animation: its screen over everything
        if let Some(export) = self.animated_export.as_mut() {
            self.canvas_rect = Rect::NOTHING;
            self.animation.reset(session);
            let ended = export.show(&ctx, &mut self.look, session);
            if let Some(ended) = ended {
                self.ended_export = self.animated_export.take();
                if matches!(ended, crate::animated_export::Ended::Failed) {
                    crate::files::tell("Share", "The animation could not be exported.");
                }
                if std::mem::take(&mut self.exporting_from_gallery) {
                    self.actions.push(Action::Gallery);
                }
            }
            session.set_backdrops(Vec::new());
            self.popover_open = false;
            return;
        }
        // the Lighting Studio: the model among its lights, and the studio's bar
        if self.lighting.open {
            self.canvas_rect = Rect::EVERYTHING;
            if let Some(notice) = self.lighting.show(&ctx, &mut self.look, session) {
                self.overlays.notice(notice, true);
            }
            self.overlays.show(ui, &mut self.look, colour, false);
            session.set_backdrops(Vec::new());
            self.popover_open = true;
            return;
        }
        // Crop and Resize: the canvas, its frame and header alone
        if self.crop.open {
            self.canvas_rect = Rect::EVERYTHING;
            self.crop.show(&ctx, &mut self.look, session);
            session.set_backdrops(Vec::new());
            self.popover_open = false;
            return;
        }
        // the drawing guide's editor: the canvas, the guide and the editor alone
        if self.guide_editor.open {
            self.canvas_rect = Rect::EVERYTHING;
            guide_lines(ui.painter(), session);
            let mut glass = Vec::new();
            self.guide_editor.show(&ctx, &mut self.look, session, &mut glass);
            session.set_backdrops(glass);
            // a handle's menu takes a press on the canvas, to close
            self.popover_open = self.guide_editor.menu_open();
            return;
        }
        // the drawing guide over the canvas, under the bars (and shown full screen too)
        guide_lines(ui.painter(), session);
        if self.bars_hidden {
            // full screen: the canvas alone (the overlays still show)
            self.canvas_rect = Rect::EVERYTHING;
            self.hover_cursor(&ctx, session, &info, input);
            self.overlays.show(ui, &mut self.look, colour, false);
            self.drop.show(&ctx, session, to_hex(self.colours.rgb()), 0.0);
            self.copy_paste_panel(&ctx, session);
            self.layer_choice(&ctx, session);
            self.quick_menu(&ctx, session);
            self.blocked_alert(&ctx, session);
            self.pdf_quality_alert(&ctx);
            self.plugin_alert(&ctx);
            self.export_card.show(&ctx, &mut self.look);
            self.touch_companion(&ctx, session);
            if self.show_interface_button(&ctx) {
                self.toggle_bars(session);
            }
            self.text_editor(&ctx, session);
            session.set_backdrops(Vec::new());
            self.popover_open = self.text.editing();
            return;
        }
        let bar = crate::chrome::top_bar(ui, &mut self.look, info.tool, colour, self.colours.hsv[2], &lit);
        let held = self.tool_hold(&ctx, session, &bar);
        match bar.pressed.filter(|_| !held) {
            Some(BarButton::Gallery) => self.actions.push(Action::Gallery),
            Some(BarButton::Actions) => self.toggle(session, Popover::Actions),
            Some(BarButton::Layers) => self.toggle(session, Popover::Layers),
            Some(b @ (BarButton::Paint | BarButton::Smudge | BarButton::Erase)) => {
                let tool = match b {
                    BarButton::Smudge => ToolKind::Smudge,
                    BarButton::Erase => ToolKind::Erase,
                    _ => ToolKind::Paint,
                };
                // leaving a mode for a tool keeps what it made, and only picks the tool; the chosen
                // tool again opens its brushes
                let goes_on = self.switch_modes(session, ModeButton::Tool);
                if goes_on && info.tool == tool && self.open.is_none_or(|o| o == Popover::Brushes) {
                    self.toggle(session, Popover::Brushes);
                } else {
                    self.choose_tool(session, tool);
                    self.open = None;
                }
            }
            Some(BarButton::Transform) => self.toggle_transform(session),
            Some(BarButton::Selection) => self.toggle_selection(session),
            Some(BarButton::Adjustments) => self.adjustments_pressed(session),
            Some(BarButton::Colour) | None => {}
        }
        // the colour button: tapped it opens the colours, held it goes back to the previous
        // colour, and dragged out its colour fills where it is let go (ColorDrop)
        if let Some(rect) = bar.rect(BarButton::Colour) {
            let id = ui.scope_id().with("colour button");
            let rgb = to_hex(self.colours.rgb());
            match self.drop.source(ui, session, id, rect, rgb, false, true, None) {
                Some(Touch::Tap) => self.toggle(session, Popover::Colour),
                Some(Touch::Hold) => {
                    if self.colours.previous(session) {
                        self.overlays.notice("Previous colour", false);
                    }
                }
                Some(Touch::Dragged) => self.open = None,
                None => {}
            }
        }
        self.canvas_rect = ui.available_rect_before_wrap();
        // the bars are glass over the canvas: the top bar's frost, the side bar's and its tracks'
        let (mut backdrops, mut windows) = (Vec::new(), Vec::new());
        let top_bar = ctx.content_rect().with_max_y(ctx.content_rect().top() + crate::panel::bar_shown(&ctx));
        crate::chrome::frost(&mut backdrops, top_bar, 0.0, ctx.pixels_per_point());
        // a mode taking the screen puts the side bar away
        let adjusting = session.adjustment_shown();
        let side_bar = self.side_bar_shown
            && !self.transform.on
            && !self.selection.on
            && session.touch_layer_opacity().is_none()
            && !self.text.editing()
            && (adjusting.adjustment.is_none() || adjusting.pencil);
        crate::panel::side_bar_at(&ctx, self.side_bar_shown, self.prefs.right_handed);
        let taps = if side_bar {
            let modify_lit = session.pick_next();
            let right = self.prefs.right_handed;
            self.side_bar.show(ui, &mut self.look, session, &info, modify_lit, right, &mut backdrops, &mut windows)
        } else {
            crate::chrome::SideTaps::default()
        };
        self.keep_brushes |= taps.brush_changed;
        if taps.undo {
            self.undo(session);
        }
        if taps.redo {
            self.redo(session);
        }
        // the modify button tapped does what Tap □ is set to (opens or closes the eyedropper at
        // first); held, touches do what □ + Touch and □ + Pen are set to (unless one did, its tap
        // then does nothing)
        // Alt held is the button held; tapped it does nothing, being in the system's own shortcuts
        let used = session.set_modify_held(taps.modify_down || input.modify_key());
        // Space tapped, not moving the canvas, opens or closes the QuickMenu (or does what a plugin
        // rebinding keys takes it as)
        let space_tapped = std::mem::take(&mut input.space_tapped);
        let to_panel = std::mem::take(&mut self.space_to_panel);
        self.space_to_panel = to_panel && !space_tapped && input.moving_view();
        if space_tapped && !to_panel && !self.keys_taken() {
            let space = KeyCombo { key: "space".into(), command: false, shift: false, alt: false };
            if let Some(press) = self.prefs.plugins.key_press(space) {
                self.press(&ctx, session, &info, &press);
            }
        }
        if taps.modify_tapped && !used {
            self.tap_modify(&ctx, session);
        }
        self.hover_cursor(&ctx, session, &info, input);
        self.selection_pointer(&ctx, input);
        if let Some(kind) = self.overlays.show(ui, &mut self.look, colour, true) {
            self.overlays.shape = session.quickshape_choose(kind).or(self.overlays.shape.take());
        }
        // the notice band frosts the canvas under it
        if let Some(r) = self.overlays.frost.take() {
            let ppp = ctx.pixels_per_point();
            backdrops.extend([r.left(), r.top(), r.right(), r.bottom(), 0.0].map(|v| v * ppp));
            backdrops.push(1.0);
        }
        // its bar tapped, the snapped shape's handles show and take drags
        session.set_shape_editing(self.overlays.shape_editing);
        let tool_button = match info.tool {
            ToolKind::Smudge => BarButton::Smudge,
            ToolKind::Erase => BarButton::Erase,
            ToolKind::Paint => BarButton::Paint,
        };
        let anchor = |p: Popover| match p {
            Popover::Actions => bar.rect(BarButton::Actions),
            Popover::Adjustments => bar.rect(BarButton::Adjustments),
            Popover::Brushes => bar.rect(tool_button),
            Popover::Layers => bar.rect(BarButton::Layers),
            Popover::Colour => bar.rect(BarButton::Colour),
        };
        // it opens as the adjustments, however it was left
        if self.open != Some(Popover::Adjustments) {
            self.adjustments.showing_filters = false;
        }
        if let Some(open) = self.open {
            self.popover(&ctx, session, open, anchor(open), &mut backdrops);
        }
        // the background's colour is kept, however its panel closed
        if self.open != Some(Popover::Layers) {
            self.end_background_pick(session);
        }
        // the colour panel dragged out by its grabber: a window over the canvas until closed, its
        // Classic as the plugins on now draw it
        if self.colours.floating.is_some() {
            self.colours.set_triangle(self.prefs.plugins.colour_triangle());
            let picker = self.prefs.plugins.picker_on().then(|| (Arc::clone(&self.prefs.plugins), Arc::clone(session)));
            self.colours.set_picker(picker);
        }
        self.colours.set_floating_look(&self.prefs.plugins);
        self.colours.show_floating(&ctx, &mut self.look, session, &mut backdrops);
        // the colour model chosen on the panel, kept; the panel works in the one kept
        if let Some(model) = self.colours.model_chosen.take() {
            self.prefs.colour_model = model;
            self.prefs.keep();
        }
        self.colours.set_model(self.prefs.colour_model);
        if self.open != Some(Popover::Actions) {
            self.actions_panel.closed();
        }
        self.transform.show(&ctx, &mut self.look, session, &mut backdrops);
        self.plugin_panel(&ctx, session, &mut backdrops);
        crate::model_picture::show(&ctx, &mut self.look, session);
        // the bars and panels that hang from the top bar hang from where it ends as it shows
        let bar_bottom = crate::panel::bar_shown(&ctx);
        if let Some(on) = self.selection.show(&ctx, &mut self.look, session, bar_bottom, self.model) {
            self.prefs.selection_x_ray = on;
            self.prefs.keep();
        }
        if let Some(asked) = self.adjustments.show(
            &ctx,
            &mut self.look,
            session,
            bar_bottom,
            &self.gradients,
            &self.colours,
            &mut backdrops,
        ) {
            self.adjustment_asked(session, asked);
        }
        let hold = std::time::Duration::from_secs_f32(self.gestures.delay(GesturePurpose::Eyedropper).max(0.0));
        let picked = self.reference.show(&ctx, &mut self.look, session, hold);
        // kept with the artwork as it changes, a toggle elsewhere last frame included
        self.reference.keep(session);
        match picked {
            Some(crate::reference::Picked::Moving { at, side, rgba, rgb }) => {
                self.overlays.pick_reference(&ctx, at, side, &rgba, rgb);
            }
            Some(crate::reference::Picked::Done) => {
                if let Some(rgb) = self.overlays.finish_pick() {
                    self.colours.set_rgb(session, rgb);
                }
            }
            None => {}
        }
        // a full-screen page covers the timeline, as it covers the canvas
        if self.canvas_info.is_none() && !self.gesture_controls.open {
            self.animation.tweens = self.prefs.plugins.tweens();
            self.animation.frame_actions = self.prefs.plugins.actions_in(procraft_core::PluginPlace::Frame);
            let said = self.animation.show(&ctx, &mut self.look, session, &self.colours.palettes, self.colours.harmony);
            if let Some(key) = self.animation.run_action.take() {
                self.run_plugin_action(&ctx, session, key);
            }
            if let Some(crate::animation::Said::NoRoom) = said {
                crate::files::no_room(session);
            }
        } else {
            self.animation.reset(session);
        }
        self.drop.show(&ctx, session, to_hex(self.colours.rgb()), bar_bottom);
        self.copy_paste_panel(&ctx, session);
        self.layer_choice(&ctx, session);
        self.quick_menu(&ctx, session);
        self.blocked_alert(&ctx, session);
        _ = self.model_warning_alert.show(&ctx, &mut self.look);
        self.pdf_quality_alert(&ctx);
        self.plugin_alert(&ctx);
        self.export_card.show(&ctx, &mut self.look);
        self.text_editor(&ctx, session);
        // a gesture setting changed: the touches follow it, and it is kept
        if self.gesture_controls.show(&ctx, &mut self.look, &self.gestures) {
            session.set_touch_settings(self.gestures.touch_settings(self.prefs.rapid_undo_ms()));
            crate::files::keep_gestures(&self.gestures.kept());
        }
        if let Some(info) = self.canvas_info.as_mut() {
            use crate::canvas_info::Ended;
            let ended = info.show(&ctx, &mut self.look, session);
            if let Some(failure) = info.take_failure() {
                self.overlays.notice(failure, true);
            }
            match ended {
                // another profile: the canvas and the current colour turned into it
                Some(Ended::Done(Some(profile))) => {
                    let (from, rgb) = (session.profile(), to_hex(self.colours.rgb()));
                    if session.change_profile(profile.clone()) {
                        self.colours.set_rgb(session, crate::canvas_info::colour_in(rgb, &from, &profile));
                    }
                    self.canvas_info = None;
                }
                Some(_) => self.canvas_info = None,
                None => {}
            }
        }
        if let Some(said) = self.time_lapse.show(&ctx, &mut self.look, &session.name()) {
            crate::files::tell("Time-lapse", said);
        }
        self.touch_companion(&ctx, session);
        self.advanced_settings(&ctx, session);
        if self.licences && crate::licences::show(&ctx) {
            self.licences = false;
        }
        let keys_to_canvas = self.canvas_shown() && !ctx.egui_wants_keyboard_input() && !self.keys_taken();
        self.shortcut_list.show(&ctx, keys_to_canvas, &self.prefs.plugins);
        self.name_colour(&ctx);
        self.place_canvas(session, ctx.pixels_per_point());
        session.set_glass(backdrops, windows);
        if input.moving_view() && !ctx.is_pointer_over_egui() {
            let icon = if input.zooming_view() { egui::CursorIcon::ZoomIn } else { egui::CursorIcon::Grab };
            ctx.set_cursor_icon(icon);
        }
        self.popover_open = self.open.is_some() || self.settings || self.text.editing();
        self.panel_lets_through = self.open.is_some_and(|p| !self.modal(p)) && !self.settings && !self.text.editing();
    }

    /// The touch companion, with its switch on in Advanced settings.
    fn touch_companion(&mut self, ctx: &egui::Context, session: &CanvasSession) {
        use crate::companion::Asked;
        if !self.prefs.touch_companion {
            return;
        }
        let mode = self.companion.mode;
        match self.companion.show(ctx, &mut self.look) {
            Some(Asked::Undo) => self.undo(session),
            Some(Asked::Redo) => self.redo(session),
            Some(Asked::Fit) => session.settle_to_fit(),
            None => {}
        }
        // its Zoom or Move turned on or off
        if self.companion.mode != mode {
            self.feedback();
        }
    }

    /// A click for an action done, with Feedback Sounds on.
    fn feedback(&self) {
        if self.prefs.feedback_sounds {
            crate::sound::click();
        }
    }

    /// Recolor on, the reticle in the middle of the window, or off (keeping what it filled).
    fn toggle_recolor(&mut self, ctx: &egui::Context, session: &CanvasSession) {
        if !session.recolor_on() && !self.takes_changes(session) {
            return;
        }
        // on, it ends; else the other modes end
        if !self.switch_modes(session, ModeButton::Recolor) {
            return;
        }
        self.open = None;
        let centre = ctx.content_rect().center() * ctx.pixels_per_point();
        self.recolor_rgb = self.colours.rgb();
        let [r, g, b] = self.recolor_rgb;
        session.recolor_start(centre.x, centre.y, r, g, b, self.prefs.recolor_threshold);
    }

    /// Layer Opacity on for the current layer (a paint layer or a group): a drag across the canvas
    /// changes its opacity, read in the band under the top bar. The other modes end.
    fn start_layer_opacity(&mut self, session: &CanvasSession) {
        use procraft_core::LayerType;
        let Some(layer) = session.layers().into_iter().find(|l| l.current) else { return };
        if !matches!(layer.kind, LayerType::Paint | LayerType::Group) {
            return;
        }
        self.switch_modes(session, ModeButton::LayerOpacity);
        self.open = None;
        session.set_touch_layer_opacity(Some(layer.id));
    }

    /// Recolor ends, keeping what it filled; its Flood kept.
    fn end_recolor(&mut self, session: &CanvasSession) {
        if session.recolor_on() {
            session.recolor_end();
            self.prefs.keep();
        }
    }

    /// Recolor, while on: its reticle and toolbar, the drags and taps on the canvas, the colour
    /// changing; a panel opening (but Colors) ends it.
    fn recolor_frame(&mut self, ctx: &egui::Context, session: &CanvasSession) {
        if !session.recolor_on() {
            return;
        }
        if self.open.is_some_and(|p| p != Popover::Colour) {
            self.end_recolor(session);
            return;
        }
        let rgb = self.colours.rgb();
        let threshold = self.prefs.recolor_threshold;
        if rgb != self.recolor_rgb {
            self.recolor_rgb = rgb;
            let [r, g, b] = rgb;
            session.recolor_colour(r, g, b);
        }
        crate::recolor::reticle(ctx, session, rgb);
        if !self.bars_hidden
            && let Some(t) = crate::adjust_panels::recolor_toolbar(ctx, threshold)
        {
            self.prefs.recolor_threshold = t;
            session.recolor_threshold(t);
        }
    }

    /// Follows the brush in use: one chosen in place of another with the same tool becomes the
    /// one Previous brush goes back to.
    fn follow_brush(&mut self, session: &CanvasSession, tool: ToolKind) {
        let b = session.current_brush();
        let now = (tool, b.set, b.index);
        if let Some((was_tool, set, index)) = self.brush_now
            && was_tool == tool
            && (set, index) != (b.set, b.index)
        {
            self.brush_last = Some((set, index));
        }
        self.brush_now = Some(now);
    }

    /// Advanced settings' page, while it is up: a change is kept and the artwork follows it.
    fn advanced_settings(&mut self, ctx: &egui::Context, session: &CanvasSession) {
        use crate::settings::Said;
        if !self.settings {
            return;
        }
        match crate::settings::show(ctx, &mut self.prefs) {
            Some(Said::Changed) => {
                self.prefs.apply(session);
                // the companion put away, its Zoom or Move goes off
                if !self.prefs.touch_companion {
                    self.companion.mode = crate::companion::Mode::None;
                }
            }
            Some(Said::Licences) => self.licences = true,
            Some(Said::Done) => self.settings = false,
            None => {}
        }
    }

    /// With Color Description Notifications, the colour named in a message as it changes: at
    /// once, or while it keeps changing, the latest at most every 150 ms.
    fn name_colour(&mut self, ctx: &egui::Context) {
        const GAP: std::time::Duration = std::time::Duration::from_millis(150);
        if !self.prefs.colour_names {
            self.named = None;
            return;
        }
        let rgb = to_hex(self.colours.rgb());
        let (last, at) = *self.named.get_or_insert((rgb, Instant::now()));
        if rgb == last {
            return;
        }
        let since = at.elapsed();
        if since >= GAP {
            self.overlays.notice(procraft_core::colour_name(rgb), false);
            self.named = Some((rgb, Instant::now()));
        } else {
            ctx.request_repaint_after(GAP - since);
        }
    }

    /// Text being edited, and the question about rasterizing a text layer.
    fn text_editor(&mut self, ctx: &egui::Context, session: &CanvasSession) {
        use crate::text::Said;
        match self.text.show(ctx, &mut self.look, session) {
            Some(Said::Notice(text, warning)) => self.overlays.notice(text, warning),
            Some(Said::Undo) => self.undo(session),
            Some(Said::Redo) => self.redo(session),
            None => {}
        }
    }

    /// New text over the canvas, in the colour chosen.
    fn add_text(&mut self, ctx: &egui::Context, session: &CanvasSession) {
        self.open = None;
        if !self.text.add(ctx, session, self.colours.rgb()) {
            crate::files::no_room(session);
        }
    }

    /// While selecting, the pointer over the canvas shows how: a fine cross where it points and
    /// the selection's kind beside it (wand, lasso, rectangle, ellipse), in place of the system's
    /// pointer. The desktop's own, as a mouse is used (the tablet shows none).
    fn selection_pointer(&mut self, ctx: &egui::Context, input: &Input) {
        if !self.selection.on || self.popover_open || !input.inside() {
            return;
        }
        let ppp = ctx.pixels_per_point();
        let Some(at) = input.cursor().map(|(x, y)| egui::pos2(x, y) / ppp) else { return };
        if !self.canvas_rect.contains(at) || ctx.layer_id_at(at).is_some_and(|l| l.order != egui::Order::Background) {
            return;
        }
        ctx.set_cursor_icon(egui::CursorIcon::None);
        let icon = self.selection.kind_icon();
        let look = &mut self.look;
        egui::Area::new(egui::Id::unique("selection pointer"))
            .order(egui::Order::Tooltip)
            .interactable(false)
            .fixed_pos(at)
            .show(ctx, |ui| {
                fine_cross(ui.painter(), at);
                // the kind below right of it, dark in a light halo (seen on any colour)
                let beside = at + egui::vec2(20.0, 20.0);
                for nudge in [(-1.5, 0.0), (1.5, 0.0), (0.0, -1.5), (0.0, 1.5)] {
                    look.icon(ui, icon, beside + egui::vec2(nudge.0, nudge.1), 26.0, Color32::WHITE);
                }
                look.icon(ui, icon, beside, 26.0, Color32::from_gray(0x20));
            });
        ctx.request_repaint();
    }

    /// The brush's outline where the mouse hovers over the canvas, as the pen's hover shows it,
    /// while nothing is pressed (and while it would paint there). Where the canvas would paint, the
    /// system's pointer gives way to the outline; where none shows (the brush too small to see,
    /// its outline none, or the brush cursor off) to a fine cross; and while a stroke is drawn to
    /// nothing, the stroke showing where the pen is (with the outline, if Prefs show it then).
    /// Painting from afar (a mouse, a pen without a screen) the eye is on the screen and the hand
    /// isn't, and the system's pointer stays where the pen went down, so the outline or the cross
    /// go on through the stroke.
    fn hover_cursor(&mut self, ctx: &egui::Context, session: &CanvasSession, info: &CanvasInfo, input: &mut Input) {
        let shown = session.adjustment_shown();
        let painting = shown.adjustment.is_none() && !self.selection.on && !self.transform.on;
        let on_canvas = |at: egui::Pos2| {
            self.canvas_rect.contains(at) && ctx.layer_id_at(at).is_none_or(|l| l.order == egui::Order::Background)
        };
        // Liquify's brush shows as a ring of its reach, hovering and pressed
        if let (Some(procraft_core::Adjustment::Liquify), Some(&size)) = (shown.adjustment, shown.liquify.first()) {
            let ppp = ctx.pixels_per_point();
            input.hover_shown = true;
            let at = input.hovering().map(|(x, y)| egui::pos2(x, y) / ppp).or_else(|| ctx.pointer_latest_pos());
            if let Some(at) = at.filter(|at| on_canvas(*at)) {
                ctx.set_cursor_icon(egui::CursorIcon::None);
                hover_ring(ctx, at, procraft_core::liquify_reach(size) * info.zoom / ppp);
            }
            return;
        }
        if !input.idle() {
            if painting
                && input.pressing()
                && let Some(at) = ctx.pointer_latest_pos().filter(|at| on_canvas(*at))
            {
                ctx.set_cursor_icon(egui::CursorIcon::None);
                if input.pressing_from_afar() && !self.overlays.outline_shown() && self.overlays.picking.is_none() {
                    hover_cross(ctx, at);
                }
            }
            return;
        }
        // the outline or the cross follows the pointer, so its moves draw a frame
        input.hover_shown = painting;
        let ppp = ctx.pixels_per_point();
        let at = input.hovering().filter(|_| painting).map(|(x, y)| egui::pos2(x, y) / ppp).filter(|at| on_canvas(*at));
        self.overlays.hover(session, info, ppp, at.filter(|_| self.prefs.cursor_shown(true)));
        if let Some(at) = at.filter(|_| !self.overlays.outline_shown()) {
            ctx.set_cursor_icon(egui::CursorIcon::None);
            hover_cross(ctx, at);
        }
    }

    /// The modify button tapped: what Tap □ is set to do, as the Android app does it.
    fn tap_modify(&mut self, ctx: &egui::Context, session: &CanvasSession) {
        match self.gestures.purpose_of(Trigger::TapModify) {
            Some(GesturePurpose::Eyedropper) => session.set_pick_next(!session.pick_next()),
            Some(GesturePurpose::Quickmenu) => self.quick_menu.toggle(ctx.content_rect().center()),
            Some(GesturePurpose::CopyPaste) => self.copy_paste = true,
            Some(GesturePurpose::Quickshape) => {
                if let Some(info) = session.quickshape_last() {
                    self.snapped(info);
                }
            }
            _ => {}
        }
    }

    /// Does what the touches asked.
    fn commands(&mut self, ctx: &egui::Context, session: &CanvasSession, input: &mut Input) {
        let mut slid = None;
        let mut sliding = None;
        let mut stroke_ended = false;
        for command in std::mem::take(&mut input.commands) {
            stroke_ended |= command == TouchCommand::StrokeEnded;
            match command {
                TouchCommand::Undo => self.undo(session),
                TouchCommand::Redo => self.redo(session),
                TouchCommand::ClearLayer => self.clear_layer(session),
                TouchCommand::ToggleBars => self.toggle_bars(session),
                TouchCommand::Fit => session.settle_to_fit(),
                TouchCommand::QuickShapeLast => {
                    if let Some(info) = session.quickshape_last() {
                        self.snapped(info);
                    }
                }
                TouchCommand::Pick { x, y } => self.overlays.pick(ctx, session, x, y),
                TouchCommand::PickDone => {
                    if let Some(rgb) = self.overlays.finish_pick() {
                        self.colours.set_rgb(session, rgb);
                    }
                }
                TouchCommand::PickCancel => self.overlays.picking = None,
                TouchCommand::ShapeSnapped => {
                    if let Some(info) = session.quickshape_info() {
                        self.snapped(info);
                    }
                }
                // a snapped shape the pen moved, dragged or snapped perfect: as it now is
                TouchCommand::ShapeChanged | TouchCommand::StrokeEnded => {
                    if self.overlays.shape.is_some() {
                        self.overlays.shape = session.quickshape_info().or(self.overlays.shape.take());
                    }
                }
                TouchCommand::ShapeEnded => {
                    self.overlays.shape = None;
                    self.overlays.shape_editing = false;
                }
                // (from afar it shows however it is set to, the hand not being on the stroke)
                TouchCommand::Cursor { x, y }
                    if self.prefs.cursor_shown(false) || (self.prefs.brush_cursor && input.pressing_from_afar()) =>
                {
                    self.overlays.cursor(ctx, session, Some((x, y)));
                }
                // not shown while painting: the outline hovering where the stroke began goes away
                // (else it stays there, left behind)
                TouchCommand::Cursor { .. } => self.overlays.cursor(ctx, session, None),
                TouchCommand::CursorGone => self.overlays.cursor(ctx, session, None),
                // a pen touching at no pressure before it presses: shown as it hovering there
                TouchCommand::PenHovers { x, y } if self.prefs.cursor_shown(true) => {
                    self.overlays.cursor(ctx, session, Some((x, y)));
                }
                TouchCommand::PenHovers { .. } => self.overlays.cursor(ctx, session, None),
                TouchCommand::FillThreshold { threshold } => {
                    self.drop.slid(threshold);
                    slid = Some(threshold);
                }
                TouchCommand::Filled => {
                    self.drop.filled();
                    slid = None;
                }
                TouchCommand::TransformTouched => self.transform.touched(),
                TouchCommand::TransformMoved { transform } => self.transform.moved(transform),
                TouchCommand::TransformHeld { handle, x, y } => self.transform.held(handle, egui::pos2(x, y)),
                TouchCommand::TransformLetGo { handle, x, y } => self.transform.let_go(handle, egui::pos2(x, y)),
                TouchCommand::SelectionDrag { x0, y0, x1, y1 } => {
                    self.selection.dragged(session, egui::pos2(x0, y0), egui::pos2(x1, y1));
                }
                TouchCommand::SelectionThreshold { x, y, threshold, done } => {
                    self.selection.slid(session, egui::pos2(x, y), threshold, done);
                    sliding = (!done).then_some((x, y, threshold));
                }
                TouchCommand::SelectionChanged => self.selection.changed(session),
                // the freehand path is read as it is drawn, each frame
                TouchCommand::FreehandChanged => {}
                // an adjustment's amount is read as it is drawn, each frame; a touch on the canvas
                // closes Layer / Pencil
                TouchCommand::AdjustChanged => self.adjustments.modes_open = false,
                TouchCommand::AdjustMenu { x, y } => {
                    self.adjustments.modes_open = false;
                    self.adjustments.menu_at = Some(egui::pos2(x, y) / ctx.pixels_per_point());
                }
                TouchCommand::TransformNumeric { angle, x, y, right } => {
                    self.transform.numbers(angle, egui::pos2(x, y) / ctx.pixels_per_point(), right);
                }
                // Recolor's reticle moved or tapped elsewhere: the fill follows at once
                TouchCommand::RecolorMoved { x, y } => session.recolor_move_to(x, y),
                TouchCommand::RecolorTapped { x, y } => session.recolor_tap(x, y),
                TouchCommand::CopyPaste => self.copy_paste = true,
                TouchCommand::LayerSelect { x, y } => self.layer_select(ctx, session, x, y),
                TouchCommand::QuickMenu { x, y } => self.quick_menu.toggle(egui::pos2(x, y) / ctx.pixels_per_point()),
                // what a pen's own settings ask its button to show
                TouchCommand::ShowColours => self.toggle(session, Popover::Colour),
                TouchCommand::ShowBrushes => self.toggle(session, Popover::Brushes),
                // a press on the canvas with a panel open: dragged or held, it goes on; a tap only
                // closes the panel
                TouchCommand::PanelFades | TouchCommand::PanelTapped => self.open = None,
                // a stroke or a fill on a text layer: it takes them once rasterized
                TouchCommand::TextLayer => self.text.ask_rasterize(),
                // (the pen's hover, which sets the brush with a finger, isn't the desktop's)
                // (the view moving on its own is drawn as it goes)
                TouchCommand::ViewMoved | TouchCommand::ViewSettles | TouchCommand::BrushChanged => {}
                // the guide editor: a touch on the canvas closes its options; the guide is read each frame
                TouchCommand::GuideTouched => self.guide_editor.touched(),
                TouchCommand::GuideChanged => {}
                TouchCommand::GuideNodeTapped { point, x, y } => {
                    self.guide_editor.node_tapped(ctx, point, egui::pos2(x, y) / ctx.pixels_per_point());
                }
            }
        }
        // a slid threshold redoes the whole fill: once a frame, at the last
        if let Some(threshold) = slid {
            session.pour_threshold(threshold);
        }
        // and so does a colour selection's
        if let Some((x, y, threshold)) = sliding {
            session.show_colour_selection(x, y, threshold);
        }
        // a stroke on a locked or hidden layer, or a group, drew nothing: say why (a text layer
        // takes none until it is rasterized, which it asks about itself)
        let on_text = session.layers().iter().any(|l| l.current && l.text);
        if stroke_ended && !on_text {
            let why = session.why_unpainted();
            self.say_blocked(session, why);
        }
    }

    /// A stroke snapped to a shape: its bar shows, and the notice says what it is.
    fn snapped(&mut self, info: procraft_core::QuickShapeInfo) {
        self.overlays.notice(snapped_to(info.kind), false);
        self.overlays.shape = Some(info);
        self.overlays.shape_editing = false;
    }

    /// Undo or redo stepped through a snapped shape: as it now is, if it is still there.
    fn follow_shape(&mut self, session: &CanvasSession) {
        self.overlays.shape = session.quickshape_info();
        self.overlays.shape_editing &= self.overlays.shape.is_some();
    }

    /// Undo (recoloring, the core takes back the open fill first).
    fn undo(&mut self, session: &CanvasSession) {
        self.feedback();
        // transforming, the transform's last change goes back, the box with it
        if self.transform.on {
            self.transform.moved(session.transform_apply(procraft_core::TransformAction::Undo));
            return;
        }
        // a finished text's step goes back to editing that text first, as a step of its own
        if !self.text.editing()
            && !self.transform.on
            && let Some(id) = session.text_undo_layer()
            && self.text.edit(session, id)
        {
            return;
        }
        // editing text, a change to its box's width goes back a step at a time, still editing
        if self.text.editing()
            && let Some(name) = session.text_box_undo()
        {
            self.text.reread(session);
            self.step_notice(session, true, Some(name));
            return;
        }
        // undoing while editing text otherwise undoes the whole edit
        self.text.end(session, true);
        let could = session.info().can_undo;
        let name = session.undo();
        if could || name.is_some() {
            self.step_notice(session, true, name);
        }
        self.follow_shape(session);
        // a model's tile chosen before comes back, with its channel's colour
        self.layers.follow_channel(session, &mut self.colours);
    }

    /// The notice an undo (`undo`) or a redo of the step `name` shows, unless a plugin hides it.
    fn step_notice(&mut self, session: &CanvasSession, undo: bool, name: Option<String>) {
        if let Some(text) = session.step_notice(undo, name) {
            self.overlays.notice(text, false);
        }
    }

    fn redo(&mut self, session: &CanvasSession) {
        self.feedback();
        // transforming, nothing is redone until it ends
        if self.transform.on {
            return;
        }
        if self.text.editing()
            && let Some(name) = session.text_box_redo()
        {
            self.text.reread(session);
            self.step_notice(session, false, Some(name));
            return;
        }
        // text being edited is kept first
        let editing = self.text.editing();
        self.text.end(session, true);
        let could = session.info().can_redo;
        // a text edit redone is edited again
        let text = if editing { None } else { session.text_redo_layer() };
        let name = session.redo();
        if could {
            self.step_notice(session, false, name);
        }
        self.follow_shape(session);
        self.layers.follow_channel(session, &mut self.colours);
        if let Some(id) = text {
            self.text.edit(session, id);
        }
    }

    fn clear_layer(&mut self, session: &CanvasSession) {
        if self.takes_changes(session) {
            session.clear_layer();
            self.overlays.notice("Layer cleared", false);
        }
    }

    /// Layer Select at window point (`x`, `y`): the top layer drawn there becomes current, and
    /// the others drawn there are listed to choose from.
    fn layer_select(&mut self, ctx: &egui::Context, session: &CanvasSession, x: f32, y: f32) {
        // on a model, the mesh there is picked
        if self.model && !session.model_flat() {
            if let Some(said) = session.pick_submesh_at(x, y) {
                self.layers.follow_picked_mesh(session);
                self.overlays.notice(said, false);
            }
            return;
        }
        let ids = session.layers_at(x, y);
        let Some(&top) = ids.first() else {
            crate::files::say("Nothing is drawn there.");
            return;
        };
        session.select_layer(top);
        self.layer_choice = (ids.len() > 1).then(|| (egui::pos2(x, y) / ctx.pixels_per_point(), ids));
    }

    /// The QuickMenu, while it shows: what a button does is done.
    fn quick_menu(&mut self, ctx: &egui::Context, session: &CanvasSession) {
        match self.quick_menu.show(ctx, &mut self.look, session, &mut self.keep_brushes) {
            Some(crate::quick_menu::Ran::Action(action, at)) => self.run_quick(ctx, session, action, at),
            // a button set to a brush gives it to the current tool
            Some(crate::quick_menu::Ran::Brush(b)) if session.select_brush(b.set, b.index) => {
                self.keep_brushes = true;
                self.library.library_changed();
            }
            _ => {}
        }
    }

    /// A QuickMenu button's action, the menu opened at `at` (points).
    fn run_quick(&mut self, ctx: &egui::Context, session: &CanvasSession, action: QuickAction, at: egui::Pos2) {
        let current = session.layers().into_iter().find(|l| l.current);
        let id = current.as_ref().map(|l| l.id);
        if let Some(a) = procraft_core::quick_action_adjustment(action) {
            self.start_adjustment(session, a);
            return;
        }
        use procraft_core::GuideType;
        let companion = |mode: crate::companion::Mode, now: crate::companion::Mode| {
            if now == mode { crate::companion::Mode::None } else { mode }
        };
        match action {
            QuickAction::Adjustments => self.adjustments_pressed(session),
            QuickAction::Liquify => self.start_adjustment(session, procraft_core::Adjustment::Liquify),
            QuickAction::Clone => self.start_adjustment(session, procraft_core::Adjustment::Clone),
            QuickAction::Guide2d => toggle_guide_kind(session, GuideType::Grid),
            QuickAction::GuideIsometric => toggle_guide_kind(session, GuideType::Isometric),
            QuickAction::GuidePerspective => toggle_guide_kind(session, GuideType::Perspective),
            QuickAction::GuideSymmetry => toggle_guide_kind(session, GuideType::Symmetry),
            QuickAction::DrawingAssist => _ = session.set_guide_assisted(!session.guide().assisted),
            QuickAction::AnimationAssist | QuickAction::PageAssist => {
                self.animation.reset(session);
                let pages = action == QuickAction::PageAssist;
                let on = session.animation_settings().is_some_and(|a| a.pages == pages);
                if pages {
                    session.set_page_assist(!on);
                } else {
                    session.set_animation_assist(!on);
                }
            }
            QuickAction::NextFrame | QuickAction::PreviousFrame => {
                // Animation Assist's next or previous frame drawn on, staying in its frames
                let frames = session.animation_frames();
                if let Some(at) = session.current_frame() {
                    let to = if action == QuickAction::NextFrame {
                        (at as usize + 1).min(frames.len().saturating_sub(1))
                    } else {
                        (at as usize).saturating_sub(1)
                    };
                    if to != at as usize
                        && let Some(&layer) = frames.get(to)
                    {
                        session.select_layer(layer);
                    }
                }
            }
            QuickAction::OnionSkin => {
                // onion skins off, or back to as many as they were
                if let Some(mut s) = session.animation_settings() {
                    if s.onion_frames > 0 {
                        self.onion_kept = s.onion_frames;
                        s.onion_frames = 0;
                    } else {
                        s.onion_frames = self.onion_kept.max(1);
                    }
                    session.set_animation_settings(s);
                }
            }
            QuickAction::BrushScaling => {
                self.prefs.brush_on_screen = !self.prefs.brush_on_screen;
                self.prefs.apply(session);
                self.prefs.keep();
            }
            QuickAction::ProjectCanvas => {
                self.prefs.project_canvas = !self.prefs.project_canvas;
                self.prefs.keep();
            }
            QuickAction::Reference => self.reference.on = !self.reference.on,
            QuickAction::Recolor => self.toggle_recolor(ctx, session),
            QuickAction::LayerOpacity => self.start_layer_opacity(session),
            QuickAction::PreviousBrush => {
                // back to the brush chosen before this one (and Previous brush comes back here)
                if let Some((set, index)) = self.brush_last
                    && session.select_brush(set, index)
                {
                    self.keep_brushes = true;
                    self.library.library_changed();
                }
            }
            QuickAction::CompanionMove => {
                self.feedback();
                self.companion.mode = companion(crate::companion::Mode::Move, self.companion.mode);
            }
            QuickAction::CompanionZoom => {
                self.feedback();
                self.companion.mode = companion(crate::companion::Mode::Zoom, self.companion.mode);
            }
            QuickAction::NewLayer => {
                if session.add_layer() == 0 {
                    crate::files::no_room(session);
                }
            }
            QuickAction::DuplicateLayer => self.duplicate_current(session),
            QuickAction::DeleteLayer => _ = id.map(|id| session.delete_layer(id)),
            QuickAction::MergeDown => _ = id.map(|id| session.merge_down(id)),
            QuickAction::ClearLayer => self.clear_layer(session),
            QuickAction::FillLayer => _ = id.map(|id| session.fill_layer(id)),
            QuickAction::SelectLayer => _ = id.map(|id| session.select_layer_contents(id)),
            QuickAction::AlphaLock => _ = current.map(|l| session.set_layer_alpha_lock(l.id, !l.alpha_lock)),
            QuickAction::LayerVisibility => _ = current.map(|l| session.tap_visibility(l.id)),
            QuickAction::Solo => {
                if let Some(said) = id.and_then(|id| session.hold_visibility(id)) {
                    self.overlays.notice(said, false);
                }
            }
            QuickAction::Copy => self.copy_layer(session),
            QuickAction::CopyCanvas => self.copy_canvas(session),
            QuickAction::Cut => self.cut_layer(session),
            QuickAction::Paste => self.paste(ctx, false),
            QuickAction::CopyPasteMenu => self.copy_paste = true,
            QuickAction::FlipHorizontal => session.flip_canvas(true),
            QuickAction::FlipVertical => session.flip_canvas(false),
            QuickAction::Undo => self.undo(session),
            QuickAction::Redo => self.redo(session),
            QuickAction::Paint | QuickAction::Smudge | QuickAction::Erase => {
                self.feedback();
                self.end_recolor(session);
                session.set_tool(match action {
                    QuickAction::Smudge => ToolKind::Smudge,
                    QuickAction::Erase => ToolKind::Erase,
                    _ => ToolKind::Paint,
                });
                self.library.tool_changed();
            }
            QuickAction::Eyedropper => session.set_pick_next(true),
            QuickAction::SwapColors => {
                if self.colours.previous(session) {
                    self.overlays.notice("Previous colour", false);
                }
            }
            QuickAction::Selection => self.toggle_selection(session),
            QuickAction::Transform => self.toggle_transform(session),
            QuickAction::Colors => self.toggle(session, Popover::Colour),
            QuickAction::Layers => self.toggle(session, Popover::Layers),
            QuickAction::Brushes => self.toggle(session, Popover::Brushes),
            QuickAction::Interface => self.toggle_bars(session),
            QuickAction::FitCanvas => session.settle_to_fit(),
            QuickAction::ActualSize => session.settle_to_actual_size(),
            QuickAction::Quickshape => {
                if let Some(info) = session.quickshape_last() {
                    self.snapped(info);
                }
            }
            QuickAction::QuickshapeOn => {
                let on = self.gestures.is_on(GesturePurpose::Quickshape, "DRAW_HOLD".into());
                self.gestures.set(GesturePurpose::Quickshape, "DRAW_HOLD".into(), !on);
                session.set_touch_settings(self.gestures.touch_settings(self.prefs.rapid_undo_ms()));
                crate::files::keep_gestures(&self.gestures.kept());
            }
            QuickAction::AddText => self.add_text(ctx, session),
            QuickAction::LayerSelect => {
                let ppp = ctx.pixels_per_point();
                self.layer_select(ctx, session, at.x * ppp, at.y * ppp);
            }
            _ => {}
        }
    }

    /// Layer Select's list, while it is up.
    fn layer_choice(&mut self, ctx: &egui::Context, session: &CanvasSession) {
        let Some((at, ids)) = &self.layer_choice else { return };
        let Some(chose) = crate::layer_choice::show(ctx, *at, ids, &session.layers()) else { return };
        if let crate::layer_choice::Chose::Layer(id) = chose {
            session.select_layer(id);
        }
        self.layer_choice = None;
    }

    /// Transform on (the current layer, or what is selected of it, in a box) or off (kept).
    /// A picture the user picks put on a new layer of its own, then transformed (a moving GIF's
    /// frames each on a layer of its own, for the timeline), as Insert a file and Insert a photo
    /// do.
    fn insert_picture(&mut self, session: &Arc<CanvasSession>, photo: bool, private: bool) {
        let title = if photo { "Insert a photo" } else { "Insert a file" };
        let Some(path) = crate::files::dialog()
            .set_title(title)
            .add_filter("Pictures and videos", &[&PICTURES[..], &VIDEOS[..]].concat())
            .pick_file()
        else {
            return;
        };
        self.insert_files(session, vec![path], private);
    }

    /// Files dropped on the canvas: a picture inserted as Insert a file does (several each a layer,
    /// in turn); brushes (ours or another app's) added to the library.
    pub fn dropped(&mut self, session: &Arc<CanvasSession>, paths: &[std::path::PathBuf]) {
        let mut brushes = Vec::new();
        let mut pictures = Vec::new();
        for path in paths {
            let extension = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            if PICTURES.contains(&extension.as_str()) || VIDEOS.contains(&extension.as_str()) {
                pictures.push(path.clone());
            } else if crate::files::is_brush_file(path)
                && let Ok(bytes) = std::fs::read(path)
            {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                brushes.push((bytes, name));
            }
        }
        self.library.add_brushes(session, brushes, true);
        if !pictures.is_empty() {
            self.open = None;
            self.insert_files(session, pictures, false);
        }
    }

    /// Brushes read from files meanwhile added to the library and told of (when asked to), the
    /// library kept. True while more are being read or wait to be added.
    fn brushes_added(&mut self, session: &CanvasSession) -> bool {
        let added = self.library.added(session, self.studio.open);
        // brushes from a file are on disk once they show in the library, not later on a thread
        // of its own: the app stopping right after (closed, or crashing) still has them
        if added.iter().any(|&(any, _)| any) {
            crate::files::keep_brushes(session);
        }
        for (any, tell) in added {
            if tell {
                let said = if any { "Brushes added to the library." } else { "This file has no brushes in it." };
                self.overlays.notice(said, false);
            } else if !any {
                crate::files::say("This file has no brushes in it.");
            }
        }
        self.keep_brushes |= std::mem::take(&mut self.library.changed);
        self.library.importing()
    }

    /// The picture in file `path` as a new layer (a private one, if `private`), ready to
    /// transform; a GIF's frames each a layer.
    fn insert_files(&mut self, session: &Arc<CanvasSession>, paths: Vec<std::path::PathBuf>, private: bool) {
        // read one after another on a thread of their own (a video's frames take seconds), what
        // each came to told on a frame after (`inserted`); a picture alone is transformed
        let transform = paths.len() == 1;
        let (session, (done, inserting)) = (Arc::clone(session), std::sync::mpsc::channel());
        std::thread::spawn(move || {
            for path in paths {
                if done.send((read_insert(&session, &path, private), transform)).is_err() {
                    return;
                }
            }
        });
        self.inserting.push(inserting);
    }

    /// What inserts read meanwhile came to: a picture alone transformed, frames told of (Animation
    /// Assist on for a video's), or why it couldn't be. True while one is still being read.
    fn inserted(&mut self, session: &CanvasSession) -> bool {
        let mut came = Vec::new();
        // each read's results so far, kept while more may come
        self.inserting.retain(|r| {
            loop {
                match r.try_recv() {
                    Ok(i) => came.push(i),
                    Err(std::sync::mpsc::TryRecvError::Empty) => break true,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => break false,
                }
            }
        });
        for (inserted, transform) in came {
            match inserted {
                Inserted::Picture(true) => {
                    if transform {
                        self.toggle_transform(session);
                    }
                }
                Inserted::Picture(false) => crate::files::no_room(session),
                Inserted::Frames { added, full, assist } => {
                    if assist {
                        self.animation.reset(session);
                        session.set_animation_assist(true);
                    }
                    if added > 0 {
                        self.overlays.notice("Frames saved to timeline", false);
                    }
                    if full {
                        crate::files::no_room(session);
                    }
                }
                Inserted::Failed(what) => crate::files::tell("Can't insert it", what),
            }
        }
        !self.inserting.is_empty()
    }

    fn toggle_transform(&mut self, session: &CanvasSession) {
        if !self.transform.on && !self.takes_changes(session) {
            return;
        }
        // on, it ends; else the other modes end
        if !self.switch_modes(session, ModeButton::Transform) {
            return;
        }
        self.open = None;
        session.set_pick_next(false);
        // nothing there to move: it says so and stays out of transform
        if let Err(said) = self.transform.begin(session) {
            self.overlays.notice(said, false);
        }
    }

    /// Opens adjustment `a` on the current layer (anything else open ends first).
    fn start_adjustment(&mut self, session: &CanvasSession, a: procraft_core::Adjustment) {
        let why = session.why_not_adjusted(a);
        if !self.say_blocked(session, why) {
            return;
        }
        self.switch_modes(session, ModeButton::Adjustment);
        self.open = None;
        session.set_pick_next(false);
        session.adjustment_start(a);
    }

    /// Opens the plugin's filter `id` on the current layer, as [`Self::start_adjustment`] does. The
    /// artwork is saved first, the first time in a session.
    /// The panel a plugin's action showed, while it is open: a touch on it told to the plugin on a
    /// thread of its own (it may take seconds), Done closing it.
    fn plugin_panel(&mut self, ctx: &egui::Context, session: &Arc<CanvasSession>, backdrops: &mut Vec<f32>) {
        let Some(panel) = self.prefs.plugins.panel() else {
            self.plugin_panel_held = None;
            return;
        };
        let asked = crate::actions::plugin_panel(ctx, &mut self.look, &panel, &mut self.plugin_panel_held, backdrops);
        match asked {
            Some(crate::actions::PanelTouch::Touch(row, value)) => {
                let (plugins, session, ctx) = (Arc::clone(&self.prefs.plugins), Arc::clone(session), ctx.clone());
                std::thread::spawn(move || {
                    plugins.panel_touched(row, value, session);
                    ctx.request_repaint();
                });
            }
            Some(crate::actions::PanelTouch::Close) => self.prefs.plugins.close_panel(),
            None => {}
        }
    }

    /// A plugin's action `key` run on the artwork, on a thread of its own (it may take seconds): the
    /// panel closes so what it does shows; anything it says, or its stopping, is told as a failure
    /// is.
    fn run_plugin_action(&mut self, ctx: &egui::Context, session: &Arc<CanvasSession>, key: String) {
        // (what it may paint on, it checks: not a locked layer)
        self.open = None;
        let (plugins, session, ctx) = (Arc::clone(&self.prefs.plugins), Arc::clone(session), ctx.clone());
        std::thread::spawn(move || {
            plugins.run_action(key, session);
            ctx.request_repaint();
        });
    }

    fn start_plugin_filter(&mut self, session: &CanvasSession, id: String) {
        if !self.takes_changes(session) {
            return;
        }
        self.switch_modes(session, ModeButton::Adjustment);
        self.open = None;
        session.set_pick_next(false);
        let plugins = std::sync::Arc::clone(&self.prefs.plugins);
        if let Some(why) = session.plugin_filter_start(plugins, id, self.artwork_dir.clone()) {
            self.overlays.notice(why, true);
        }
    }

    /// Ends the open adjustment: `commit` keeps it (and says so), otherwise the layer goes back.
    fn end_adjustment(&mut self, session: &CanvasSession, commit: bool) {
        self.adjustments.menu_at = None;
        if session.adjustment_end(commit) && commit {
            self.overlays.notice("Adjustment applied", false);
        }
    }

    /// The adjustments button: closes an open adjustment (keeping it), or opens the menu.
    fn adjustments_pressed(&mut self, session: &CanvasSession) {
        if self.switch_modes(session, ModeButton::Adjustments) {
            self.toggle(session, Popover::Adjustments);
        }
    }

    /// What the tap menu asked of the open adjustment.
    fn adjustment_asked(&mut self, session: &CanvasSession, asked: crate::adjustments::Asked) {
        use crate::adjustments::Asked;
        match asked {
            Asked::Apply => {
                session.adjustment_apply();
                self.overlays.notice("Adjustment applied", false);
            }
            Asked::Undo => session.adjustment_undo(),
            Asked::Preview => session.adjustment_preview(!session.adjustment_shown().preview),
            Asked::Cancel => self.end_adjustment(session, false),
            Asked::Reset => session.adjustment_reset(),
        }
    }

    /// Selection on (touches select, as its toolbar says) or off (a freehand path drawn is closed).
    fn toggle_selection(&mut self, session: &CanvasSession) {
        // on, it ends; else the other modes end
        if !self.switch_modes(session, ModeButton::Selection) {
            return;
        }
        self.open = None;
        session.set_pick_next(false);
        self.selection.open(session);
    }

    /// The Copy & Paste panel, while it is up: what was chosen there is done, and it goes.
    fn copy_paste_panel(&mut self, ctx: &egui::Context, session: &CanvasSession) {
        if !self.copy_paste {
            return;
        }
        let can_paste = self.can_paste();
        let Some(choice) = crate::copy_paste::show(ctx, &mut self.look, can_paste) else { return };
        self.copy_paste = false;
        match choice {
            Choice::Close => {}
            Choice::Cut => self.cut_layer(session),
            Choice::Copy => self.copy_layer(session),
            Choice::CopyAll => self.copy_canvas(session),
            Choice::Duplicate => self.duplicate_current(session),
            // straight back as a new layer: what is on the clipboard stays there
            Choice::CutAndPaste => {
                if let Some(cut) = self.layer_cut(session) {
                    self.paste_image(session, cut, false);
                }
            }
            Choice::Paste => self.paste(ctx, false),
        }
    }

    /// The current layer (or what is selected of it) cut, to paste.
    fn cut_layer(&mut self, session: &CanvasSession) {
        if let Some(cut) = self.layer_cut(session) {
            self.keep_copy(session, cut);
        }
    }

    /// The current layer (or what is selected of it) cut; none if nothing was.
    fn layer_cut(&mut self, session: &CanvasSession) -> Option<procraft_core::ThumbnailImage> {
        self.open = None;
        // one layer at a time: a group current or several chosen say so
        let why = session.why_not_copied(true);
        if !self.say_blocked(session, why) {
            return None;
        }
        let cut = session.cut_layer()?;
        self.overlays.notice("Layer cut", false);
        Some(cut)
    }

    /// The current layer (or what is selected of it) copied, to paste.
    fn copy_layer(&mut self, session: &CanvasSession) {
        if let Some(copied) = self.layer_copied(session) {
            self.keep_copy(session, copied);
        }
    }

    /// The current layer (or what is selected of it) copied; none if nothing was.
    fn layer_copied(&mut self, session: &CanvasSession) -> Option<procraft_core::ThumbnailImage> {
        self.open = None;
        // one layer at a time: a group current or several chosen say so
        let why = session.why_not_copied(false);
        if !self.say_blocked(session, why) {
            return None;
        }
        let copied = session.copy_layer()?;
        self.overlays.notice("Layer copied", false);
        Some(copied)
    }

    /// The whole picture (or what is selected of it) copied, to paste.
    fn copy_canvas(&mut self, session: &CanvasSession) {
        self.open = None;
        if let Some(copied) = session.copy_canvas() {
            self.keep_copy(session, copied);
            self.overlays.notice("Canvas copied", false);
        }
    }

    /// What was cut or copied (`copy`), kept to paste in place of what was before: here as it is,
    /// and on the system's clipboard as a picture for other apps.
    fn keep_copy(&mut self, session: &CanvasSession, copy: procraft_core::ThumbnailImage) {
        self.system_clipboard.copied_here(&copy, session.copy_profile());
        self.clipboard = Some(copy);
    }

    /// Whether there is anything for Paste to take, as far as is known.
    fn can_paste(&self) -> bool {
        self.clipboard.is_some() || self.system_clipboard.offers
    }

    /// Paste (on a new private layer, one the gallery's thumbnail leaves out, if `private`): what
    /// was copied last, here or in another app. The system's clipboard is read first, so what
    /// comes of it comes on a later frame (`pasted`).
    fn paste(&mut self, ctx: &egui::Context, private: bool) {
        self.open = None;
        self.system_clipboard.paste(private);
        ctx.request_repaint();
    }

    /// What Paste found on the system's clipboard, once it has: another app's picture comes in as
    /// an inserted one does, ready to transform. True while a look at it is under way.
    fn pasted(&mut self, session: &Arc<CanvasSession>) -> bool {
        match self.system_clipboard.take_pasted() {
            Some(Pasted::Own { private }) => self.paste_own(session, private),
            // (a mask that takes what is pasted has no use for a copy of the picture)
            Some(Pasted::Picture { image, private: false }) if session.paste_picture_into_mask(image.clone()) => {}
            Some(Pasted::Picture { image, private }) => {
                if self.model && !session.model_flat() {
                    if !session.model_picture_begin(image, true) {
                        crate::files::no_room(session);
                    }
                } else if session.insert_image(image, private).is_some() {
                    self.toggle_transform(session);
                } else {
                    crate::files::no_room(session);
                }
            }
            Some(Pasted::Files { paths, private }) => self.insert_files(session, paths, private),
            None => {}
        }
        self.system_clipboard.busy()
    }

    /// What was copied or cut here, pasted as a new layer (a private one, if `private`).
    fn paste_own(&mut self, session: &CanvasSession, private: bool) {
        self.open = None;
        let Some(image) = self.clipboard.clone() else { return };
        self.paste_image(session, image, private);
    }

    /// A copy (`image`) pasted as a new layer (a private one, if `private`).
    fn paste_image(&mut self, session: &CanvasSession, image: procraft_core::ThumbnailImage, private: bool) {
        self.open = None;
        if self.model && !session.model_flat() {
            if !session.model_picture_begin(image, false) {
                crate::files::no_room(session);
            }
            return;
        }
        let pasted = if private { session.paste_private(image) } else { session.paste(image) };
        if pasted.is_none() {
            crate::files::no_room(session);
        }
    }

    /// What is selected copied and pasted as a new layer (what is on the clipboard stays there);
    /// nothing selected, the layer duplicated.
    fn duplicate_current(&mut self, session: &CanvasSession) {
        if session.has_selection() {
            if let Some(copied) = self.layer_copied(session) {
                self.paste_image(session, copied, false);
            }
        } else if let Some(current) = session.layers().into_iter().find(|l| l.current)
            && session.duplicate_layer(current.id).is_none()
        {
            crate::files::no_room(session);
        }
    }

    /// Whether the current layer takes a change; if not (locked, hidden or left out of the
    /// picture, or an empty group), an alert says why. What was tried is not done afterwards.
    fn takes_changes(&mut self, session: &CanvasSession) -> bool {
        let why = session.why_blocked();
        self.say_blocked(session, why)
    }

    /// As [`Self::takes_changes`], for why the current layer took nothing (`why`, if anything).
    fn say_blocked(&mut self, session: &CanvasSession, why: Option<Blocked>) -> bool {
        let Some(why) = why else { return true };
        // Liquify's alert is about the layer chosen whose alpha is locked
        let layers = session.layers();
        let alpha_locked = (why == Blocked::AlphaLocked).then(|| session.alpha_locked_chosen()).flatten();
        let shown = layers.iter().find(|l| Some(l.id) == alpha_locked).or_else(|| layers.iter().find(|l| l.current));
        let Some(l) = shown.cloned() else { return true };
        let (title, message, icon, extra) = match why {
            Blocked::Locked => (
                "Layer is locked",
                format!("\u{201c}{}\u{201d} can't be changed while it is locked.", l.name),
                &crate::icons::PAGE_LOCKED,
                Some("Unlock"),
            ),
            Blocked::Hidden | Blocked::GroupHidden => (
                if why == Blocked::Hidden { "Layer is hidden" } else { "Group is hidden" },
                hidden_message(&l.name, session.hidden_by()),
                &crate::icons::EYE_STRUCK,
                Some("Show"),
            ),
            Blocked::Group => (
                "Group is selected",
                format!("\u{201c}{}\u{201d} is a group: choose one of the layers in it.", l.name),
                &crate::icons::FOLDER,
                None,
            ),
            Blocked::Several => (
                "Several layers are chosen",
                "This works on one layer at a time: choose just one.".to_owned(),
                &crate::icons::FOLDER,
                None,
            ),
            Blocked::EmptyGroup => (
                "Group is empty",
                format!("\u{201c}{}\u{201d} holds no layers to change.", l.name),
                &crate::icons::FOLDER,
                None,
            ),
            Blocked::MeshHidden => (
                "Mesh is hidden",
                format!(
                    "\u{201c}{}\u{201d} can't be painted while it is hidden.",
                    session.meant_mesh_name().unwrap_or_default()
                ),
                &crate::icons::EYE_STRUCK,
                Some("Show"),
            ),
            Blocked::AlphaLocked => (
                "Alpha Lock is on",
                format!("\u{201c}{}\u{201d} can't be liquified while its alpha is locked.", l.name),
                &crate::icons::PAGE_LOCKED,
                Some("Turn Off"),
            ),
            Blocked::MeshLocked => (
                "Mesh is locked",
                format!(
                    "\u{201c}{}\u{201d} can't be painted while it is locked.",
                    session.meant_mesh_name().unwrap_or_default()
                ),
                &crate::icons::PAGE_LOCKED,
                Some("Unlock"),
            ),
        };
        self.blocked = Some((why, l.id));
        self.blocked_alert.ask(Asking {
            title: title.into(),
            message,
            action: "Open Layers".into(),
            icon: Some(icon),
            destructive: false,
            extra: extra.map(Into::into),
            first: None,
            alone: false,
        });
        false
    }

    /// The PDF quality alert, while it shows: the PDF made at the quality picked.
    fn pdf_quality_alert(&mut self, ctx: &egui::Context) {
        let Some(answer) = self.pdf_alert.show(ctx, &mut self.look) else { return };
        let Some(format) = self.pdf_for.take() else { return };
        if let Some(quality) = crate::alert::pdf_jpeg(answer) {
            self.actions.push(Action::Export(format, quality));
            self.open = None;
        }
    }

    /// Asks whether to import the plugin file at `path`: nothing in it runs, and nothing is kept
    /// until the answer comes. What can't be imported says why.
    pub fn offer_plugin_file(&mut self, path: &std::path::Path) {
        let cant = "Can't import this plugin";
        let Some(bytes) = procraft_core::read_plugin_file(path) else {
            self.plugin_alert.tell(cant, "This file can't be read.", None);
            return;
        };
        let offer = self.prefs.plugins.inspect(bytes.clone());
        if let Some(why) = offer.refused {
            self.plugin_alert.tell(cant, &why, None);
            return;
        }
        self.plugin_file = Some(bytes);
        self.plugin_alert.ask(Asking {
            title: offer.name,
            message: offer.message,
            action: "Import".to_owned(),
            icon: None,
            destructive: false,
            extra: None,
            first: None,
            alone: false,
        });
    }

    /// A plugin file's screen, while it shows: answered, the file is imported, or it was only
    /// being told something.
    pub fn plugin_alert(&mut self, ctx: &egui::Context) {
        if let Some(said) = self.prefs.plugins.take_moved_notice() {
            self.plugin_alert.tell("Now a plugin", &said, None);
        }
        let Some(answer) = self.plugin_alert.show(ctx, &mut self.look) else { return };
        let Some(bytes) = self.plugin_file.take().filter(|_| answer == Answer::Action) else { return };
        match self.prefs.plugins.install(bytes) {
            Some(why) => self.plugin_alert.tell("Can't import this plugin", &why, None),
            None => self.prefs.keep(),
        }
    }

    /// Says what a model newly opened may not paint well with.
    fn model_warnings(&mut self, session: &CanvasSession) {
        let warnings = session.take_model_warnings();
        if warnings.is_empty() {
            return;
        }
        let names: Vec<String> = warnings.into_iter().map(procraft_core::model_warning_name).collect();
        self.model_warning_alert.ask(Asking {
            title: "Unsupported model features".into(),
            message: format!("Painting on this model may not go as expected because of:\n{}", names.join("\n")),
            action: "OK".into(),
            icon: None,
            destructive: false,
            extra: None,
            first: None,
            alone: true,
        });
    }

    /// The blocked alert, while it shows: unlock or show the layer, or open Layers.
    fn blocked_alert(&mut self, ctx: &egui::Context, session: &CanvasSession) {
        let Some(answer) = self.blocked_alert.show(ctx, &mut self.look) else { return };
        let Some((why, id)) = self.blocked.take() else { return };
        match answer {
            Answer::Extra if why == Blocked::Locked => _ = session.set_layer_locked(id, false),
            Answer::Extra if why == Blocked::AlphaLocked => _ = session.set_layer_alpha_lock(id, false),
            // its own box ticked (what else hides it stays)
            Answer::Extra if matches!(why, Blocked::Hidden | Blocked::GroupHidden) => {
                if let Some(said) = session.show_layer(id) {
                    self.overlays.notice(said, false);
                }
            }
            Answer::Extra if matches!(why, Blocked::MeshHidden | Blocked::MeshLocked) => session.free_meant_meshes(),
            Answer::Action => self.open = Some(Popover::Layers),
            _ => {}
        }
    }

    /// The interface hides (full screen) or comes back; hiding, it closes its panels.
    /// A tool button held (as long as a long press): that tool takes the brush the current one has
    /// (erasing with the painting brush, say) and becomes the tool. True while the press holding
    /// it has acted, so that its letting go doesn't tap it too.
    fn tool_hold(&mut self, ctx: &egui::Context, session: &CanvasSession, bar: &crate::chrome::Bar) -> bool {
        let tool = |b: BarButton| match b {
            BarButton::Paint => Some(ToolKind::Paint),
            BarButton::Smudge => Some(ToolKind::Smudge),
            BarButton::Erase => Some(ToolKind::Erase),
            _ => None,
        };
        if let Some((target, since)) = bar.down.and_then(|(b, since)| tool(b).map(|t| (t, since)))
            && !self.tool_hold_acted
        {
            let left = TOOL_HOLD_S - (ctx.input(|i| i.time) - since);
            if left <= 0.0 {
                self.tool_hold_acted = true;
                self.tool_held(session, target);
            } else {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(left));
            }
        }
        let acted = self.tool_hold_acted;
        if bar.down.is_none() {
            self.tool_hold_acted = false;
        }
        acted
    }

    /// The tool `target` takes the current brush, as its button held does.
    fn tool_held(&mut self, session: &CanvasSession, target: ToolKind) {
        let brush = session.current_brush();
        if session.info().tool == target || !self.switch_modes(session, ModeButton::ToolHeld) {
            return;
        }
        self.choose_tool(session, target);
        if session.select_brush(brush.set, brush.index) {
            self.keep_brushes = true;
        }
        if self.open == Some(Popover::Brushes) {
            self.open = None;
        }
        let doing = match target {
            ToolKind::Paint => "Painting",
            ToolKind::Smudge => "Smudging",
            ToolKind::Erase => "Erasing",
        };
        self.overlays.notice(format!("{doing} with the current brush"), false);
    }

    /// With the interface hidden, a button in the top left (44 square, 20 in) that brings it
    /// back: the full screen glyph, drawn wider than it is tall (36.7 by 32). True when tapped.
    fn show_interface_button(&mut self, ctx: &egui::Context) -> bool {
        let screen = ctx.content_rect();
        let button = Rect::from_min_size(screen.left_top() + vec2(20.0, 0.0), vec2(44.0, 44.0));
        let mut tapped = false;
        egui::Area::new(egui::Id::unique("show the interface"))
            .fixed_pos(button.min)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                tapped = ui.allocate_rect(button, egui::Sense::click()).clicked();
                let glyph = Rect::from_center_size(button.center() + vec2(0.0, 0.3), vec2(36.7, 32.0));
                self.look.icon_in(ui, &crate::icons::FULL_SCREEN, glyph, argb(pc::SIDE_GLYPH));
            });
        tapped
    }

    /// The interface hidden or shown again (not while transforming).
    fn toggle_bars(&mut self, session: &CanvasSession) {
        if !self.switch_modes(session, ModeButton::Bars) {
            return;
        }
        self.bars_hidden = !self.bars_hidden;
        if self.bars_hidden {
            self.open = None;
        }
    }

    /// A panel's button: the panel opens (or closes), what works on the canvas ending first.
    fn toggle(&mut self, session: &CanvasSession, popover: Popover) {
        self.feedback();
        self.switch_modes(session, ModeButton::Panel { colour: popover == Popover::Colour });
        // the colour button with the panel floating: it goes back in its place
        if popover == Popover::Colour && self.colours.floating.take().is_some() {
            self.open = None;
        }
        self.open = if self.open == Some(popover) { None } else { Some(popover) };
    }

    /// The modes open now, as the core's [`mode_switch`] takes them.
    fn open_modes(&self, session: &CanvasSession) -> Modes {
        Modes {
            quickshape: self.overlays.shape.is_some(),
            filling: self.drop.filling(),
            recolor: session.recolor_on(),
            layer_opacity: session.touch_layer_opacity().is_some(),
            transform: self.transform.on,
            selection: self.selection.on,
            adjustment: session.adjustment_shown().adjustment.is_some(),
        }
    }

    /// `button` pressed: the modes it leaves behind end first (keeping what they made, but for
    /// the selection its own button lets go of), as the core says; true if it goes on to do what
    /// it does.
    fn switch_modes(&mut self, session: &CanvasSession, button: ModeButton) -> bool {
        let open = self.open_modes(session);
        let ModeSwitch { end, goes_on } = mode_switch(button, open);
        if mode_switch_deselects(button, open, self.selection.color_fill()) {
            self.selection.clear(session);
        }
        if end.quickshape {
            session.quickshape_done();
            self.overlays.shape = None;
            self.overlays.shape_editing = false;
        }
        if end.filling {
            self.drop.end_filling();
        }
        if end.recolor {
            self.end_recolor(session);
        }
        if end.layer_opacity {
            session.set_touch_layer_opacity(None);
        }
        if end.transform {
            // kept, it ends the selection too
            self.transform.end(session, true);
            self.selection.changed(session);
        }
        if end.selection {
            self.selection.close(session);
        }
        if end.adjustment {
            self.end_adjustment(session, true);
        }
        goes_on
    }

    /// The colour panel picking the background's colour, hanging from the Layers button: what is
    /// picked goes to the background as it changes, the brush keeping its colour; its tick closes
    /// it. Returns where it is.
    fn background_panel(
        &mut self,
        ctx: &egui::Context,
        session: &Arc<CanvasSession>,
        button: Rect,
        backdrops: &mut Vec<f32>,
    ) -> Rect {
        let Some(pick) = self.background_pick.as_mut() else { return Rect::NOTHING };
        self.colours.set_triangle(self.prefs.plugins.colour_triangle());
        let picker = self.prefs.plugins.picker_on().then(|| (Arc::clone(&self.prefs.plugins), Arc::clone(session)));
        self.colours.set_picker(picker);
        let brush = std::mem::replace(&mut self.colours.hsv, pick.hsv);
        self.colours.background = true;
        let history = self.prefs.colour_history_shown;
        let shown = self.colours.show(
            ctx,
            &mut self.look,
            session,
            &mut self.drop,
            button,
            &mut self.prefs.colour_history_shown,
            backdrops,
        );
        if self.prefs.colour_history_shown != history {
            self.prefs.keep();
        }
        self.colours.background = false;
        let picked = std::mem::replace(&mut self.colours.hsv, brush);
        if let Some(rgb) = pick.follow(picked) {
            let [r, g, b] = of_hex(rgb);
            session.set_background_color(r, g, b, false);
        }
        if std::mem::take(&mut self.colours.background_done) {
            self.open = None;
        }
        shown
    }

    /// The background's colour picked is kept, as one undo step (if it changed), and the panel
    /// goes back to the brush's.
    fn end_background_pick(&mut self, session: &CanvasSession) {
        if let Some(rgb) = self.background_pick.take().and_then(BackgroundPick::kept) {
            let [r, g, b] = of_hex(rgb);
            session.set_background_color(r, g, b, true);
        }
    }

    /// The open panel, hanging from its button; a press anywhere else, or Escape, closes it.
    fn popover(
        &mut self,
        ctx: &egui::Context,
        session: &Arc<CanvasSession>,
        which: Popover,
        button: Option<Rect>,
        backdrops: &mut Vec<f32>,
    ) {
        let Some(button) = button else {
            self.open = None;
            return;
        };
        // while one of the panel's menus is open, a press elsewhere only closes the menu
        let modal = self.modal(which);
        // laid out in the panels' own space, hanging from where the button shows, and shown at the
        // size a plugin gives the panels
        let k = crate::panel::part_scale(ctx, procraft_core::InterfacePart::Panels);
        let space = crate::panel::PanelSpace::new(ctx.content_rect(), crate::panel::bar_shown(ctx), k);
        crate::panel::drawing_panel(ctx, Some(space));
        let (on_window, button) = (button, space.to_window.inverse().mul_rect(button));
        let shown = match which {
            Popover::Brushes => {
                let shown = self.library.show(ctx, &mut self.look, session, button, backdrops);
                // what was done in the library is kept
                self.keep_brushes |= std::mem::take(&mut self.library.changed);
                shown
            }
            Popover::Layers if self.background_pick.is_some() => self.background_panel(ctx, session, button, backdrops),
            Popover::Layers => {
                self.layers.plugin_actions = self.prefs.plugins.actions_in(procraft_core::PluginPlace::Layer);
                self.layers.layer_kinds = self.prefs.plugins.layer_kinds();
                let shown = self.layers.show(ctx, &mut self.look, session, &mut self.colours, button, backdrops);
                if let Some(key) = self.layers.run_action.take() {
                    self.run_plugin_action(ctx, session, key);
                }
                if let Some((text, warning)) = self.layers.notice.take() {
                    self.overlays.notice(text, warning);
                }
                if let Some(copied) = self.layers.copied.take() {
                    self.keep_copy(session, copied);
                }
                if std::mem::take(&mut self.layers.share) {
                    self.actions.push(Action::Export(Format::ChosenLayerPngs, 100));
                }
                if let Some(id) = self.layers.edit_text.take() {
                    self.open = None;
                    self.text.edit(session, id);
                }
                if std::mem::take(&mut self.layers.layer_opacity) {
                    self.start_layer_opacity(session);
                }
                // its background row tapped: the colour panel in its place, on the background's
                if std::mem::take(&mut self.layers.pick_background) {
                    let b = session.background();
                    self.background_pick = Some(BackgroundPick::new(to_hex([b.r, b.g, b.b]), self.colours.hsv[0]));
                }
                shown
            }
            Popover::Colour => {
                self.colours.set_triangle(self.prefs.plugins.colour_triangle());
                let picker =
                    self.prefs.plugins.picker_on().then(|| (Arc::clone(&self.prefs.plugins), Arc::clone(session)));
                self.colours.set_picker(picker);
                let shown = self.prefs.colour_history_shown;
                let history = &mut self.prefs.colour_history_shown;
                let rect = self.colours.show(ctx, &mut self.look, session, &mut self.drop, button, history, backdrops);
                if self.prefs.colour_history_shown != shown {
                    self.prefs.keep();
                }
                // dragged out, it floats instead
                if self.colours.floating.is_some() {
                    self.open = None;
                }
                rect
            }
            Popover::Adjustments => {
                use crate::adjustments::MenuRow;
                let filters = self.prefs.plugins.filters();
                let actions = self.prefs.plugins.actions_in(procraft_core::PluginPlace::Adjustments);
                let turned = self.adjustments.showing_filters && (!filters.is_empty() || !actions.is_empty());
                let (shown, tapped) =
                    crate::adjustments::menu(ctx, &mut self.look, button, backdrops, &filters, &actions, turned);
                match tapped {
                    Some(MenuRow::Open(a)) => self.start_adjustment(session, a),
                    Some(MenuRow::Plugins) => self.adjustments.showing_filters = true,
                    Some(MenuRow::Back) => self.adjustments.showing_filters = false,
                    Some(MenuRow::Filter(id)) => {
                        self.adjustments.showing_filters = false;
                        self.start_plugin_filter(session, id);
                    }
                    Some(MenuRow::Action(key)) => {
                        self.adjustments.showing_filters = false;
                        self.run_plugin_action(ctx, session, key);
                    }
                    None => {}
                }
                shown
            }
            Popover::Actions => {
                let assist = session.animation_settings();
                let showing = crate::actions::Showing {
                    can_paste: self.can_paste(),
                    animation: assist.as_ref().is_some_and(|a| !a.pages),
                    pages: assist.as_ref().is_some_and(|a| a.pages),
                    time_lapse: self.time_lapse.recording(),
                    time_lapse_recorded: self.time_lapse.recorded(),
                    reference: self.reference.on,
                    guide: session.guide().on,
                    side_bar: self.side_bar_shown,
                    model: self.model,
                    model_flat: self.model && session.model_flat(),
                    paint_through: self.model && session.paint_through(),
                    svg: session.exports_svg(),
                    plugin_exports: session.plugin_exports().into_iter().map(|e| e.label).collect(),
                };
                let (shown, act) =
                    self.actions_panel.show(ctx, &mut self.look, button, &showing, &mut self.prefs, backdrops);
                match act {
                    Some(Act::ImportPlugin) => {
                        if let Some(path) = crate::files::pick_plugin() {
                            self.offer_plugin_file(&path);
                        }
                    }
                    Some(Act::PluginAction) => {
                        if let Some(key) = self.actions_panel.take_plugin_action() {
                            self.run_plugin_action(ctx, session, key);
                        }
                    }
                    Some(Act::Prefs) => {
                        self.prefs.apply(session);
                        session.set_touch_settings(self.gestures.touch_settings(self.prefs.rapid_undo_ms()));
                        self.prefs.keep();
                    }
                    Some(Act::SideBar) => self.side_bar_shown = !self.side_bar_shown,
                    Some(Act::TimeLapseReplay) => {
                        self.open = None;
                        self.time_lapse.replay();
                    }
                    Some(Act::TimeLapseRecording) => self.time_lapse.set_recording(!self.time_lapse.recording()),
                    Some(Act::ExportTimeLapse) => {
                        self.open = None;
                        self.time_lapse.ask_length();
                    }
                    Some(Act::AnimatedExport(format)) => {
                        self.open = None;
                        self.animated_export = Some(crate::animated_export::AnimatedExport::new(format));
                    }
                    Some(act @ (Act::AnimationAssist | Act::PageAssist)) => {
                        self.animation.reset(session);
                        let pages = act == Act::PageAssist;
                        let on = session.animation_settings().is_some_and(|a| a.pages == pages);
                        if pages {
                            session.set_page_assist(!on);
                        } else {
                            session.set_animation_assist(!on);
                        }
                    }
                    Some(Act::Licences) => self.licences = true,
                    Some(Act::Shortcuts) => {
                        self.open = None;
                        self.shortcut_list.open = true;
                    }
                    Some(Act::ModelFlat) => session.set_model_flat(!session.model_flat()),
                    Some(Act::ModelAnimated(format)) => {
                        self.open = None;
                        self.animated_export = Some(crate::animated_export::AnimatedExport::model(format));
                    }
                    Some(Act::PaintThrough) => session.set_paint_through(!session.paint_through()),
                    Some(Act::LightingStudio) => {
                        self.open = None;
                        self.lighting.begin(session);
                    }
                    Some(Act::AdvancedSettings) => {
                        self.open = None;
                        self.settings = true;
                    }
                    Some(Act::Flip { horizontal }) => session.flip_canvas(horizontal),
                    Some(Act::AddText) => self.add_text(ctx, session),
                    Some(
                        act @ (Act::InsertFile | Act::InsertPhoto | Act::InsertPrivateFile | Act::InsertPrivatePhoto),
                    ) => {
                        self.open = None;
                        let photo = matches!(act, Act::InsertPhoto | Act::InsertPrivatePhoto);
                        let private = matches!(act, Act::InsertPrivateFile | Act::InsertPrivatePhoto);
                        self.insert_picture(session, photo, private);
                    }
                    Some(Act::Share(format @ (Format::Pdf | Format::LayersPdf))) => {
                        // how well it keeps its pictures, asked first
                        self.pdf_for = Some(format);
                        self.pdf_alert.ask(crate::alert::pdf_quality());
                    }
                    Some(Act::Share(format)) => {
                        self.actions.push(Action::Export(format, 100));
                        self.open = None;
                    }
                    Some(Act::PluginExport(i)) => {
                        self.actions.push(Action::PluginExport(i));
                        self.open = None;
                    }
                    Some(Act::About) => crate::files::about(),
                    Some(Act::GestureControls) => {
                        self.open = None;
                        self.gesture_controls.open = true;
                    }
                    Some(Act::Reference) => self.reference.on = !self.reference.on,
                    Some(Act::DrawingGuide) => _ = session.set_guide_on(!session.guide().on),
                    Some(Act::EditGuide) => {
                        self.open = None;
                        self.guide_editor.begin(session);
                    }
                    Some(Act::Crop) => {
                        self.open = None;
                        self.crop.begin(session);
                    }
                    Some(Act::CanvasInfo) => {
                        self.open = None;
                        self.canvas_info = Some(crate::canvas_info::CanvasInfo::new(
                            session,
                            self.artwork_dir.clone(),
                            self.time_lapse.info(),
                            self.time_lapse.settings(),
                        ));
                    }
                    Some(Act::Cut) => self.cut_layer(session),
                    Some(Act::Copy) => self.copy_layer(session),
                    Some(Act::CopyCanvas) => self.copy_canvas(session),
                    Some(Act::Paste) => self.paste(ctx, false),
                    Some(Act::PastePrivate) => self.paste(ctx, true),
                    // the panel does these itself
                    Some(
                        Act::Page(_)
                        | Act::ScalingMenu
                        | Act::TipAttachment
                        | Act::LightInterface
                        | Act::RightHanded
                        | Act::BrushCursor
                        | Act::ProjectCanvas
                        | Act::ResetInput,
                    )
                    | None => {}
                }
                shown
            }
        };
        crate::panel::drawing_panel(ctx, None);
        if !modal {
            self.close_outside(ctx, space.to_window.mul_rect(shown), on_window);
        }
    }

    /// One of the menus of popover `which` is open.
    fn modal(&self, which: Popover) -> bool {
        match which {
            Popover::Layers if self.background_pick.is_some() => self.colours.modal(),
            Popover::Layers => self.layers.modal(),
            Popover::Brushes => self.library.modal(),
            Popover::Colour => self.colours.modal(),
            Popover::Actions => self.licences || self.actions_panel.modal(),
            Popover::Adjustments => false,
        }
    }

    /// A press anywhere but on the popover at `shown` and its `button`, or Escape, closes it (one
    /// the canvas took closes it as the touches say).
    fn close_outside(&mut self, ctx: &egui::Context, shown: Rect, button: Rect) {
        let pressed_outside = !self.canvas_pressed
            && ctx.input(|i| {
                i.pointer.any_pressed()
                    && i.pointer.interact_pos().is_some_and(|p| !shown.contains(p) && !button.contains(p))
            });
        if escape(ctx) || pressed_outside {
            self.open = None;
        }
    }

    /// A hex keypad, or text being edited, takes the keys (a text field's are egui's own).
    fn keys_taken(&self) -> bool {
        self.text.editing()
            || ((self.open == Some(Popover::Colour)
                || self.colours.floating.is_some()
                || self.background_pick.is_some())
                && self.colours.takes_keys())
            || self.animation.takes_keys()
    }

    /// Nothing covers the canvas: no full-screen page or editor is open.
    fn canvas_shown(&self) -> bool {
        !(self.studio.open
            || self.time_lapse.replaying()
            || self.animated_export.is_some()
            || self.lighting.open
            || self.crop.open
            || self.guide_editor.open
            || self.gesture_controls.open
            || self.canvas_info.is_some()
            || self.settings)
    }

    /// A hardware keyboard's shortcuts: the core's (keys.rs), and the desktop's own: Ctrl (⌘) with
    /// N, O, S (and Shift) for its files, with +, - and 0 for its view. A plugin rebinding keys takes
    /// each press as it says first; a plugin's panel asking for a key press takes the next.
    fn shortcuts(&mut self, ctx: &egui::Context, session: &Arc<CanvasSession>, info: &CanvasInfo) {
        // a text field, or the hex keypad, takes the keys; text being edited takes them all
        if ctx.egui_wants_keyboard_input() || self.keys_taken() {
            return;
        }
        let keys: Vec<(Key, Modifiers, bool)> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Key { key, pressed: true, modifiers, repeat, .. } => Some((*key, *modifiers, *repeat)),
                    _ => None,
                })
                .collect()
        });
        let mut asking = self.prefs.plugins.panel().is_some_and(|panel| panel.key_press);
        for (key, modifiers, repeat) in keys {
            let Some(name) = key_name(key) else { continue };
            let press =
                KeyCombo { key: name.into(), command: modifiers.command, shift: modifiers.shift, alt: modifiers.alt };
            if asking {
                if !repeat {
                    // the one press it asked for, told on a thread of its own (it may take a while)
                    asking = false;
                    match key {
                        Key::Escape => _ = escape(ctx),
                        Key::Space => self.space_to_panel = true,
                        _ => {}
                    }
                    let (plugins, session, ctx) = (Arc::clone(&self.prefs.plugins), Arc::clone(session), ctx.clone());
                    std::thread::spawn(move || {
                        plugins.panel_key(press, session);
                        ctx.request_repaint();
                    });
                }
                ctx.input_mut(|i| i.consume_key(modifiers, key));
                continue;
            }
            // R tapped turns the canvas a step, held it turns it by dragging, and Space tapped opens
            // the QuickMenu, held it moves the canvas: the window's input sees to them (input.rs),
            // as to Escape and F11 elsewhere
            if key == Key::R && !modifiers.command && !modifiers.alt {
                ctx.input_mut(|i| i.consume_key(modifiers, key));
                continue;
            }
            if matches!(key, Key::Space | Key::Escape | Key::F11) {
                continue;
            }
            let did = match self.prefs.plugins.key_press(press) {
                Some(press) => self.press(ctx, session, info, &press),
                // a key a plugin took away does nothing
                None => true,
            };
            if did {
                ctx.input_mut(|i| i.consume_key(modifiers, key));
            }
        }
    }

    /// Does what `press` does (as a plugin rebinding keys took it): the desktop's own, else the
    /// core's shortcut on it. True if it did something.
    fn press(&mut self, ctx: &egui::Context, session: &CanvasSession, info: &CanvasInfo, press: &KeyCombo) -> bool {
        if press.command {
            // Ctrl (⌘) with + or = zooms in about the window's middle, with - out, with 0 fits the
            // canvas (the desktop's own, as the R key's turn)
            let middle = ctx.content_rect().center() * ctx.pixels_per_point();
            match press.key.as_str() {
                "s" => self.actions.push(if press.shift { Action::SaveAs } else { Action::Save }),
                "n" => self.actions.push(Action::New),
                "o" => self.actions.push(Action::Open),
                "=" | "plus" => session.view_gesture(middle.x, middle.y, 0.0, 0.0, procraft_core::KEY_ZOOM, 0.0),
                "-" => session.view_gesture(middle.x, middle.y, 0.0, 0.0, 1.0 / procraft_core::KEY_ZOOM, 0.0),
                "0" => session.settle_to_fit(),
                _ => {
                    return self.canvas_press(ctx, session, info, press);
                }
            }
            return true;
        }
        self.canvas_press(ctx, session, info, press)
    }

    /// Does the core's shortcut on `press`, if it has one; true if it did.
    fn canvas_press(
        &mut self,
        ctx: &egui::Context,
        session: &CanvasSession,
        info: &CanvasInfo,
        press: &KeyCombo,
    ) -> bool {
        let shortcut = procraft_core::key_shortcut(
            press.key.clone(),
            press.command,
            press.shift,
            press.alt,
            self.transform.on || self.selection.nudges(),
        );
        let Some(shortcut) = shortcut else { return false };
        self.key_shortcut(ctx, session, info, shortcut);
        true
    }

    /// Does what a keyboard shortcut says.
    fn key_shortcut(&mut self, ctx: &egui::Context, session: &CanvasSession, info: &CanvasInfo, shortcut: KeyShortcut) {
        let middle = ctx.content_rect().center();
        match shortcut {
            KeyShortcut::Undo => self.undo(session),
            KeyShortcut::Redo => self.redo(session),
            KeyShortcut::CopyLayer => self.copy_layer(session),
            KeyShortcut::CutLayer => self.cut_layer(session),
            KeyShortcut::Paste => self.paste(ctx, false),
            KeyShortcut::Paint => self.tool_key(session, ToolKind::Paint),
            KeyShortcut::Erase => self.tool_key(session, ToolKind::Erase),
            KeyShortcut::Eyedropper => session.set_pick_next(!session.pick_next()),
            KeyShortcut::Selection => self.toggle_selection(session),
            KeyShortcut::Transform => self.toggle_transform(session),
            KeyShortcut::HueSaturationBrightness => self.start_adjustment(session, procraft_core::Adjustment::Hsb),
            KeyShortcut::Colours => self.toggle(session, Popover::Colour),
            KeyShortcut::Actions => self.toggle(session, Popover::Actions),
            KeyShortcut::Layers => self.toggle(session, Popover::Layers),
            KeyShortcut::SwapColours => self.colours.swap(session),
            KeyShortcut::Deselect => self.selection.clear(session),
            KeyShortcut::DuplicateLayer => self.duplicate_current(session),
            KeyShortcut::Interface => self.toggle_bars(session),
            KeyShortcut::ClearLayer => self.clear_layer(session),
            KeyShortcut::CopyCanvas => self.run_quick(ctx, session, QuickAction::CopyCanvas, middle),
            KeyShortcut::ColourBalance => self.run_quick(ctx, session, QuickAction::ColorBalance, middle),
            KeyShortcut::PerspectiveGuide => self.run_quick(ctx, session, QuickAction::GuidePerspective, middle),
            KeyShortcut::QuickMenu => self.quick_menu.toggle(middle),
            KeyShortcut::GreyView => session.set_grey_view(!session.grey_view()),
            KeyShortcut::BrushSize { by } => session.set_brush_size((info.size + by).clamp(0.0, 1.0)),
            KeyShortcut::Nudge { dx, dy } if self.transform.on => self.transform.moved(session.transform_arrow(dx, dy)),
            // the strokes the selection takes, and the selection with them
            KeyShortcut::Nudge { dx, dy } => {
                session.nudge_plugin_items(dx, dy);
            }
            // the view's own moves a plugin took a press as
            KeyShortcut::ZoomView { .. }
            | KeyShortcut::TurnView { .. }
            | KeyShortcut::PanView { .. }
            | KeyShortcut::FitView => {
                session.key_view(shortcut);
            }
        }
    }

    /// A tool's key: whatever mode is open ends, keeping what it made, as for the tool's button.
    fn tool_key(&mut self, session: &CanvasSession, tool: ToolKind) {
        self.switch_modes(session, ModeButton::Tool);
        self.choose_tool(session, tool);
    }

    /// A tool chosen (as its button does when it isn't the current one).
    fn choose_tool(&mut self, session: &CanvasSession, tool: ToolKind) {
        self.feedback();
        self.end_recolor(session);
        session.set_tool(tool);
        self.library.tool_changed();
    }
}

/// Whether Escape was pressed this frame, for what is open to close: it counts as taken, so it
/// doesn't also turn the canvas upright ([`Interface::show`]).
pub fn escape(ctx: &egui::Context) -> bool {
    let pressed = ctx.input(|i| i.key_pressed(Key::Escape));
    if pressed {
        let frame = ctx.cumulative_pass_nr();
        ctx.data_mut(|d| d.insert_temp(escape_id(), frame));
    }
    pressed
}

/// Escape was pressed this frame, and nothing open took it.
fn escape_untaken(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.key_pressed(Key::Escape))
        && ctx.data(|d| d.get_temp::<u64>(escape_id())) != Some(ctx.cumulative_pass_nr())
}

fn escape_id() -> egui::Id {
    egui::Id::unique("escape taken")
}

/// The name the core's shortcuts know `key` by (keys.rs, as a plugin is told a press), if it has
/// one.
fn key_name(key: Key) -> Option<&'static str> {
    Some(match key {
        Key::OpenBracket | Key::OpenCurlyBracket => "[",
        Key::CloseBracket | Key::CloseCurlyBracket => "]",
        Key::Semicolon => ";",
        Key::Delete => "delete",
        Key::Backspace => "backspace",
        Key::ArrowLeft => "left",
        Key::ArrowRight => "right",
        Key::ArrowUp => "up",
        Key::ArrowDown => "down",
        Key::Space => "space",
        Key::Escape => "escape",
        Key::Minus => "-",
        Key::Equals => "=",
        Key::Plus => "plus",
        Key::Comma => ",",
        Key::Period => ".",
        Key::Slash => "/",
        Key::Backslash => "\\",
        Key::Quote => "'",
        Key::Backtick => "`",
        Key::F1 => "f1",
        Key::F2 => "f2",
        Key::F3 => "f3",
        Key::F4 => "f4",
        Key::F5 => "f5",
        Key::F6 => "f6",
        Key::F7 => "f7",
        Key::F8 => "f8",
        Key::F9 => "f9",
        Key::F10 => "f10",
        Key::F11 => "f11",
        Key::F12 => "f12",
        // the letters and digits by their names ("A", "0")
        _ => Some(key.name()).filter(|n| n.len() == 1)?,
    })
}

/// The drawing guide on as `kind`, or off if it already is that.
fn toggle_guide_kind(session: &CanvasSession, kind: procraft_core::GuideType) {
    let g = session.guide();
    if g.on && g.kind == kind {
        session.set_guide_on(false);
        return;
    }
    if !g.on {
        session.set_guide_on(true);
    }
    session.set_guide_kind(kind);
}

/// The drawing guide's lines, painted on `painter`: the main panel's own, before the top bar is, so
/// that the bar (and everything over the canvas) shows above them, as the canvas under it does.
fn guide_lines(painter: &egui::Painter, session: &CanvasSession) {
    let g = session.guide();
    if !g.on {
        return;
    }
    let ppp = painter.ctx().pixels_per_point();
    let [r, gr, b] = [0, 1, 2].map(|k| (g.color.get(k).copied().unwrap_or(0.0).clamp(0.0, 1.0) * 255.0).round() as u8);
    let colour = Color32::from_rgba_unmultiplied(r, gr, b, (g.opacity.clamp(0.0, 1.0) * 255.0).round() as u8);
    let stroke = Stroke::new(0.25 + 1.5 * g.thickness, colour);
    for &[x0, y0, x1, y1] in g.lines.as_chunks::<4>().0 {
        painter.line_segment([egui::pos2(x0, y0) / ppp, egui::pos2(x1, y1) / ppp], stroke);
    }
}

/// The fine cross over the canvas at `at` (points), above all else.
fn hover_cross(ctx: &egui::Context, at: egui::Pos2) {
    egui::Area::new(egui::Id::unique("hover cross"))
        .order(egui::Order::Tooltip)
        .interactable(false)
        .fixed_pos(at)
        .show(ctx, |ui| fine_cross(ui.painter(), at));
}

/// A fine ring `radius` points round `at`, a cross at its middle, seen on any colour.
fn hover_ring(ctx: &egui::Context, at: egui::Pos2, radius: f32) {
    egui::Area::new(egui::Id::unique("hover ring")).order(egui::Order::Tooltip).interactable(false).fixed_pos(at).show(
        ctx,
        |ui| {
            for (width, colour) in [(3.0, Color32::WHITE), (1.0, Color32::BLACK)] {
                ui.painter().circle_stroke(at, radius, Stroke::new(width, colour));
            }
            fine_cross(ui.painter(), at);
        },
    );
}

/// The desktop's pointer over the canvas where no outline shows: a fine cross at `at` (points),
/// dark on a light edge, seen on any colour.
fn fine_cross(painter: &egui::Painter, at: egui::Pos2) {
    for (width, colour) in [(3.0, Color32::WHITE), (1.0, Color32::BLACK)] {
        let stroke = Stroke::new(width, colour);
        painter.line_segment([at - vec2(7.0, 0.0), at + vec2(7.0, 0.0)], stroke);
        painter.line_segment([at - vec2(0.0, 7.0), at + vec2(0.0, 7.0)], stroke);
    }
}

fn style(ctx: &egui::Context) {
    crate::look::install_fonts(ctx);
    ctx.set_theme(egui::Theme::Dark);
    ctx.style_mut_of(egui::Theme::Dark, |s| {
        s.visuals.selection.bg_fill = argb(pc::ACCENT);
        s.visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
        s.visuals.hyperlink_color = argb(pc::ACCENT);
        // the fields' cursor blue, as the Android app's, seen on the dark and the light interface
        s.visuals.text_cursor.stroke = Stroke::new(2.0, argb(pc::ACCENT));
        s.visuals.window_fill = POPOVER_FILL;
        s.visuals.panel_fill = crate::look::argb(pc::BAR);
        s.visuals.slider_trailing_fill = true;
        s.spacing.button_padding = vec2(10.0, 5.0);
        s.spacing.item_spacing = vec2(8.0, 6.0);
    });
}

/// What an insert read on its own thread came to.
enum Inserted {
    /// A picture on a layer of its own (to transform), or none for want of room.
    Picture(bool),
    /// Frames each on a layer of its own (a moving picture's or a video's): how many, whether the
    /// canvas ran out of room, and whether Animation Assist comes on with them.
    Frames { added: u32, full: bool, assist: bool },
    /// It couldn't be read.
    Failed(&'static str),
}

/// The file at `path` brought onto the canvas (private layers, if `private`): a video's frames
/// each fitted on a layer of their own at the animation's frame rate (or 15 a second; for each
/// time a frame falls due, the video's frame nearest it; 999 at most), a moving picture's frames,
/// or a picture on a layer of its own.
fn read_insert(session: &CanvasSession, path: &std::path::Path, private: bool) -> Inserted {
    let extension = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if VIDEOS.contains(&extension.as_str()) {
        let settings = session.animation_settings();
        let fps = f64::from(settings.as_ref().map_or(VIDEO_FPS, |a| a.fps).clamp(1, 60));
        let (mut added, mut full, mut start) = (0u32, false, None);
        let read = crate::video::read_frames(path, |rgba, width, height, at| {
            // its time from the first frame's (a video may start late); the frames falling due
            // nearer it than the next one's take it
            let at = at - *start.get_or_insert(at);
            while !full && added < MOST_VIDEO_FRAMES && at + 0.5 / fps + 1e-3 >= f64::from(added) / fps {
                let image = procraft_core::ThumbnailImage { width, height, rgba: rgba.to_vec() };
                if session.insert_image(image, private).is_some() {
                    added += 1;
                } else {
                    full = true;
                }
            }
            !full && added < MOST_VIDEO_FRAMES
        });
        return match (added, read) {
            (0, Ok(_)) if full => Inserted::Frames { added: 0, full: true, assist: false },
            (0, Err(e)) if e == crate::video::NO_FFMPEG => Inserted::Failed(crate::video::NO_FFMPEG),
            (0, _) => Inserted::Failed("This video can't be opened."),
            _ => Inserted::Frames { added, full, assist: settings.is_none_or(|a| a.pages) },
        };
    }
    let Ok(bytes) = std::fs::read(path) else { return Inserted::Failed("This picture can't be opened.") };
    if let Some(done) = session.insert_gif_frames(bytes.clone(), private) {
        if done.added == 0 && !done.full {
            return Inserted::Failed("This picture can't be opened.");
        }
        return Inserted::Frames { added: done.added, full: done.full, assist: false };
    }
    match image::load_from_memory(&bytes) {
        Ok(picture) => {
            let rgba = picture.to_rgba8();
            let image =
                procraft_core::ThumbnailImage { width: rgba.width(), height: rgba.height(), rgba: rgba.into_raw() };
            let on_model = session.model_info().is_some() && !session.model_flat();
            Inserted::Picture(if on_model {
                session.model_picture_begin(image, true)
            } else {
                session.insert_image(image, private).is_some()
            })
        }
        Err(_) => Inserted::Failed("This picture can't be opened."),
    }
}

#[cfg(test)]
mod tests {
    use egui::{Event, Pos2, RawInput, pos2};
    use winit::dpi::PhysicalPosition;
    use winit::event::{DeviceId, ElementState, Force, MouseButton, Touch, TouchPhase, WindowEvent};

    use super::*;

    /// The interface over a canvas in a window (a pixel a point), its input handed on as the app's
    /// window hands it on: a touch to egui (the first one down its pointer as well, as egui-winit
    /// makes it) and to the canvas, which takes it only off the interface; the mouse to both; keys
    /// to egui, each with the modifiers held as it came.
    struct Window {
        ctx: egui::Context,
        ui: Interface,
        input: Input,
        session: Arc<CanvasSession>,
        touches: crate::input::InterfaceTouches,
        size: egui::Vec2,
        time: f64,
        events: Vec<Event>,
        pointer_touch: Option<u64>,
    }

    impl Window {
        /// A canvas opened on a headless GPU in a window `size` points across, the side bar on the
        /// right with `right`; none without a GPU.
        fn open(size: egui::Vec2, right: bool) -> Option<Window> {
            Window::open_with(size, crate::prefs::Prefs { right_handed: right, ..Default::default() })
        }

        /// As [`Window::open`], with `prefs`.
        fn open_with(size: egui::Vec2, prefs: crate::prefs::Prefs) -> Option<Window> {
            let gpu = procraft_gpu::Gpu::headless().map_err(|e| eprintln!("skipped: {e}")).ok()?;
            let session = CanvasSession::new(400, 300);
            session.set_display_scale(1.0);
            session.use_gpu(gpu);
            session.show_in(size.x as u32, size.y as u32);
            let ctx = egui::Context::default();
            crate::look::install_fonts(&ctx);
            let mut ui = Interface::with_prefs(prefs);
            ui.opened(&session, 1.0);
            let (input, touches) = (Input::default(), Default::default());
            let mut window =
                Window { ctx, ui, input, session, touches, size, time: 0.0, events: Vec::new(), pointer_touch: None };
            for _ in 0..3 {
                window.frame();
            }
            Some(window)
        }

        fn frame(&mut self) {
            self.time += 1.0 / 60.0;
            self.input.tick(&self.session);
            let raw = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)),
                time: Some(self.time),
                events: std::mem::take(&mut self.events),
                ..RawInput::default()
            };
            let Window { ctx, ui, input, session, .. } = self;
            let mut output = ctx.clone().run_ui(raw, |egui_ui| ui.show(egui_ui, session, input));
            output.textures_delta.clear();
        }

        /// A window event to the canvas, which leaves what lands on the interface to it.
        fn canvas_hears(&mut self, event: &WindowEvent) {
            let canvas = self.ui.canvas_rect;
            let blocked = self.ui.popover_open && !self.ui.panel_lets_through;
            let ctx = &self.ctx;
            self.input
                .on_event(&self.session, event, false, |p| blocked || crate::screen::on_interface(ctx, p, canvas));
        }

        /// The pen touching at `at`, as the system's pointer messages bring it.
        fn pen(&mut self, phase: TouchPhase, at: Pos2) {
            const PEN: u64 = 77;
            crate::pen::pointer_kind(PEN, true);
            let location = PhysicalPosition::new(f64::from(at.x), f64::from(at.y));
            let touch =
                Touch { device_id: DeviceId::dummy(), phase, location, force: Some(Force::Normalized(0.5)), id: PEN };
            let canvas = self.ui.canvas_rect;
            let ctx = &self.ctx;
            for heard in self.touches.route(WindowEvent::Touch(touch), |p| crate::screen::on_interface(ctx, p, canvas))
            {
                if let WindowEvent::Touch(t) = heard {
                    self.egui_touch(&t);
                }
            }
            self.canvas_hears(&WindowEvent::Touch(touch));
        }

        /// A touch as egui-winit gives it to egui: as itself, and as the pointer while it is the
        /// first one down.
        fn egui_touch(&mut self, t: &Touch) {
            let at = Pos2::new(t.location.x as f32, t.location.y as f32);
            let phase = match t.phase {
                TouchPhase::Started => egui::TouchPhase::Start,
                TouchPhase::Moved => egui::TouchPhase::Move,
                TouchPhase::Ended => egui::TouchPhase::End,
                TouchPhase::Cancelled => egui::TouchPhase::Cancel,
            };
            let id = egui::TouchId::from(t.id);
            self.events.push(Event::Touch { device_id: egui::TouchDeviceId(0), id, phase, pos: at, force: Some(0.5) });
            if self.pointer_touch.is_some_and(|id| id != t.id) {
                return;
            }
            let button = |pressed| Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            match t.phase {
                TouchPhase::Started => {
                    self.pointer_touch = Some(t.id);
                    self.events.extend([Event::PointerMoved(at), button(true)]);
                }
                TouchPhase::Moved => self.events.push(Event::PointerMoved(at)),
                TouchPhase::Ended => {
                    self.pointer_touch = None;
                    self.events.extend([button(false), Event::PointerGone]);
                }
                TouchPhase::Cancelled => {
                    self.pointer_touch = None;
                    self.events.push(Event::PointerGone);
                }
            }
        }

        /// The mouse pointer moving to `at` (a pen hovering moves it too).
        fn hover(&mut self, at: Pos2) {
            self.events.push(Event::PointerMoved(at));
            let position = PhysicalPosition::new(f64::from(at.x), f64::from(at.y));
            self.canvas_hears(&WindowEvent::CursorMoved { device_id: DeviceId::dummy(), position });
            self.frame();
        }

        /// A pen stroke across the middle of the window, on the canvas.
        fn stroke(&mut self) {
            let from = Pos2::new(self.size.x / 2.0 - 60.0, self.size.y / 2.0);
            self.pen(TouchPhase::Started, from);
            self.frame();
            for i in 1..=20 {
                self.pen(TouchPhase::Moved, from + vec2(i as f32 * 6.0, 0.0));
                self.frame();
            }
            self.pen(TouchPhase::Ended, from + vec2(120.0, 0.0));
            self.frame();
        }

        /// The pen hovering to `at` and tapping there, sliding a little as it lands.
        fn pen_tap(&mut self, at: Pos2) {
            self.hover(at);
            self.pen(TouchPhase::Started, at);
            self.frame();
            self.pen(TouchPhase::Moved, at + vec2(2.0, 2.0));
            self.pen(TouchPhase::Ended, at + vec2(2.0, 2.0));
            self.frame();
            self.frame();
        }

        /// The mouse clicking at `at`.
        fn click(&mut self, at: Pos2) {
            self.hover(at);
            for (pressed, state) in [(true, ElementState::Pressed), (false, ElementState::Released)] {
                let button = egui::PointerButton::Primary;
                self.events.push(Event::PointerButton { pos: at, button, pressed, modifiers: Modifiers::NONE });
                self.canvas_hears(&WindowEvent::MouseInput {
                    device_id: DeviceId::dummy(),
                    state,
                    button: MouseButton::Left,
                });
                self.frame();
            }
            self.frame();
        }

        /// `key` pressed and let go with Ctrl (and Shift) held, as a keyboard sends it or a tablet's
        /// driver for one of its buttons.
        fn ctrl_key(&mut self, key: Key, shift: bool) {
            let modifiers = Modifiers { ctrl: true, command: true, shift, ..Modifiers::NONE };
            for pressed in [true, false] {
                self.events.push(Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers });
            }
            self.frame();
            self.frame();
        }

        /// Whether there is a step to undo, and one to redo.
        fn steps(&self) -> (bool, bool) {
            let info = self.session.info();
            (info.can_undo, info.can_redo)
        }

        /// The middle of the side bar's undo button, and of its redo button, as the side bar shows
        /// `scale` times as big under a top bar shown `bar` times as big.
        fn undo_redo(&self, scale: f32, bar: f32) -> (Pos2, Pos2) {
            let window = Rect::from_min_size(Pos2::ZERO, self.size);
            let fit = crate::chrome::fit_side_bar(window, self.ui.prefs.right_handed, scale, pc::BAR_HEIGHT * bar);
            let (laid_out, to_window) = (fit.laid_out(), fit.to_window());
            let shorter = pc::SLIDER_HEIGHT - fit.track;
            let at = |y: f32| to_window * Pos2::new(laid_out.center().x, laid_out.top() + y - shorter * 2.0);
            (at(451.0), at(493.0))
        }

        /// The middle of the top bar's tool `button` (or Layers, or the colour), as the bar shows
        /// `scale` times as big.
        fn tool_button(&self, button: BarButton, scale: f32) -> Pos2 {
            let width = self.size.x / scale;
            let (tool_gap, _) = pc::bar_gaps(width);
            let from_colour = match button {
                BarButton::Colour => 0.0,
                BarButton::Layers => 1.0,
                BarButton::Erase => 2.0,
                BarButton::Smudge => 3.0,
                _ => 4.0,
            };
            let x =
                width - pc::BAR_MARGIN - pc::BAR_BUTTON_WIDTH / 2.0 - from_colour * (pc::BAR_BUTTON_WIDTH + tool_gap);
            Pos2::new(x, pc::BAR_HEIGHT / 2.0) * scale
        }

        /// Where the area `id` (its layer `order`) shows, and how it is drawn onto the window.
        fn area_shown(&self, order: egui::Order, id: egui::Id) -> Option<(Rect, egui::emath::TSTransform)> {
            let rect = self.ctx.memory(|m| m.area_rect(id))?;
            let layer = egui::LayerId::new(order, id);
            let to_window = self.ctx.layer_transform_to_global(layer).unwrap_or(egui::emath::TSTransform::IDENTITY);
            Some((to_window.mul_rect(rect), to_window))
        }
    }

    /// The app's plugins in a folder of their own, the Interface plugin sizing each part of the
    /// interface separately: `scales` in the parts' order (floating panels, menus, the side bar, the
    /// top bar, the panels).
    fn parts_sized(name: &str, scales: [f32; 5]) -> (Arc<procraft_core::Plugins>, PathBuf) {
        const INTERFACE: &str = "io.github.artcraftandroid.interface";
        let dir = std::env::temp_dir().join(format!("procraft-parts-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let plugins = procraft_core::Plugins::with_bundled(dir.to_string_lossy().into_owned());
        assert!(plugins.set_on(INTERFACE.to_owned(), true));
        assert!(plugins.set_switch(format!("{INTERFACE}.size_on"), true));
        assert!(plugins.set_switch(format!("{INTERFACE}.parts"), true));
        for (key, scale) in ["floating", "menus", "side_bar", "top_bar", "panels"].into_iter().zip(scales) {
            assert!(plugins.set_slider(format!("{INTERFACE}.{key}"), scale).is_some(), "{key}");
        }
        (plugins, dir)
    }

    #[test]
    fn the_side_bar_and_the_top_bar_bigger_or_smaller_take_presses_where_they_show() {
        for (scales, right) in [([1.0, 1.0, 1.5, 1.5, 1.0], false), ([1.0, 1.0, 0.75, 1.25, 1.0], true)] {
            let (plugins, dir) = parts_sized(&format!("bars-{right}"), scales);
            let prefs = crate::prefs::Prefs { right_handed: right, plugins, ..Default::default() };
            let Some(mut w) = Window::open_with(vec2(1200.0, 800.0), prefs) else { return };
            let (side, bar) = (scales[2], scales[3]);
            w.stroke();
            assert_eq!(w.steps(), (true, false), "{scales:?}");
            // the side bar's undo and redo, where they show
            let (undo, redo) = w.undo_redo(side, bar);
            w.click(undo);
            assert_eq!(w.steps(), (false, true), "{scales:?}");
            w.click(redo);
            assert_eq!(w.steps(), (true, false), "{scales:?}");
            // its layer draws it at its size, from its top left under the top bar as it shows
            let (shown, to_window) = w.area_shown(egui::Order::Middle, egui::Id::unique("side bar")).unwrap();
            assert_eq!(to_window.scaling, side);
            assert!((shown.width() - pc::SIDE_BAR_WIDTH * side).abs() < 1e-3, "{shown:?}");
            assert!(shown.top() >= pc::BAR_HEIGHT * bar, "{shown:?}");
            // the top bar's Layers button, where it shows, opens the layers
            w.click(w.tool_button(BarButton::Layers, bar));
            assert_eq!(w.ui.open, Some(Popover::Layers), "{scales:?}");
            // the canvas starts under the bar as it shows
            assert_eq!(w.ui.canvas_rect.top(), pc::BAR_HEIGHT * bar);
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn a_panel_bigger_or_smaller_hangs_from_its_button_where_the_top_bar_shows_it_and_its_menus_stay_their_size() {
        for (bar, panels) in [(1.5, 1.5), (1.0, 0.5), (1.25, 1.0)] {
            let (plugins, dir) = parts_sized(&format!("panels-{bar}-{panels}"), [1.0, 1.0, 1.0, bar, panels]);
            let prefs = crate::prefs::Prefs { plugins, ..Default::default() };
            let Some(mut w) = Window::open_with(vec2(1200.0, 800.0), prefs) else { return };
            let button = w.tool_button(BarButton::Layers, bar);
            w.click(button);
            assert_eq!(w.ui.open, Some(Popover::Layers));
            // (an area new to egui is guessed big and kept to the window at first, then settles)
            for _ in 0..40 {
                w.frame();
            }
            let case = format!("{bar} {panels}");
            let id = egui::Id::unique("layers panel");
            let (shown, to_window) = w.area_shown(egui::Order::Foreground, id).unwrap();
            assert_eq!(to_window.scaling, panels, "{case}");
            assert!((shown.top() - pc::BAR_HEIGHT * bar).abs() < 1e-3, "{case}: {shown:?}");
            assert!((shown.width() - crate::layers::LAYERS_WIDTH * panels).abs() < 1e-3, "{case}: {shown:?}");
            assert!(shown.left() < button.x && button.x < shown.right(), "{case}: {shown:?}");
            assert!(shown.bottom() <= 800.0, "{case}: {shown:?}");
            // its title opens its menu, at the menus' size, from where the title shows
            let laid_out = w.ctx.memory(|m| m.area_rect(id)).unwrap();
            let title = pos2(laid_out.left() + 24.0 + 20.0, laid_out.top() + crate::panel::ARROW_HEIGHT + 19.0 + 12.0);
            w.click(to_window * title);
            std::thread::sleep(std::time::Duration::from_millis(350));
            w.frame();
            let menu = egui::Id::unique(("layers title menu", "menu"));
            let (_, menu_to_window) = w.area_shown(egui::Order::Tooltip, menu).expect("the title's menu");
            assert!((menu_to_window.scaling - 1.0).abs() < 1e-3, "{case}: {menu_to_window:?}");
            // (the area's place is rounded to the pixel)
            let opened = w.ctx.memory(|m| m.area_rect(menu)).unwrap();
            let pivot = pos2(opened.center().x, opened.top());
            assert!(((menu_to_window * pivot) - (to_window * pivot)).length() < 0.5, "{case}");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn the_side_bar_s_redo_takes_a_pen_s_tap_and_a_click_after_its_undo_as_ctrl_shift_z_does() {
        // as measured, and with its tracks shortened to fit a short window on the right
        for (size, right) in [(vec2(1200.0, 800.0), false), (vec2(1000.0, 420.0), true)] {
            let Some(mut w) = Window::open(size, right) else { return };
            w.stroke();
            assert_eq!(w.steps(), (true, false), "{size:?}");
            let (undo, redo) = w.undo_redo(1.0, 1.0);
            // a tablet's pen through the system's pointer messages (Windows Ink)
            w.pen_tap(undo);
            assert_eq!(w.steps(), (false, true), "{size:?}");
            w.pen_tap(redo);
            assert_eq!(w.steps(), (true, false), "{size:?}");
            // the mouse (a tablet's pen moves it with Windows Ink off)
            w.click(undo);
            assert_eq!(w.steps(), (false, true), "{size:?}");
            w.click(redo);
            assert_eq!(w.steps(), (true, false), "{size:?}");
            // the keys, from a keyboard or a pen's button; Ctrl+Y redoes too off a Mac, as Windows
            // and Linux have it and tablets' express keys send it (a Mac keeps ⇧⌘Z alone)
            w.ctrl_key(Key::Z, false);
            assert_eq!(w.steps(), (false, true), "{size:?}");
            w.ctrl_key(Key::Y, false);
            let mac = cfg!(target_os = "macos");
            assert_eq!(w.steps(), (!mac, mac), "{size:?}");
            if !mac {
                w.ctrl_key(Key::Z, false);
                assert_eq!(w.steps(), (false, true), "{size:?}");
            }
            w.ctrl_key(Key::Z, true);
            assert_eq!(w.steps(), (true, false), "{size:?}");
        }
    }

    #[test]
    fn keys_go_by_the_names_the_core_knows_and_go_as_they_are_with_no_plugin() {
        let named = [
            (Key::Z, "Z"),
            (Key::Num0, "0"),
            (Key::OpenCurlyBracket, "["),
            (Key::Space, "space"),
            (Key::Escape, "escape"),
            (Key::Plus, "plus"),
            (Key::Equals, "="),
            (Key::Backslash, "\\"),
            (Key::F1, "f1"),
            (Key::F12, "f12"),
            (Key::ArrowUp, "up"),
        ];
        for (key, name) in named {
            assert_eq!(key_name(key), Some(name), "{key:?}");
        }
        assert_eq!(key_name(Key::Tab), None);
        assert_eq!(key_name(Key::F13), None);
        // with no plugin rebinding keys, each press goes as it is (its key as the core names it)
        let plugins = procraft_core::Plugins::new();
        let press = KeyCombo { key: "Z".into(), command: true, shift: false, alt: false };
        assert_eq!(plugins.key_press(press).map(|p| (p.key, p.command)), Some(("z".to_owned(), true)));
        assert_eq!(plugins.key_shortcut_list(true, false), procraft_core::key_shortcut_list(true, false));
    }

    #[test]
    fn a_background_pick_sends_each_change_and_keeps_the_last_as_it_closes() {
        // white, a grey, starts on the brush's hue
        let mut pick = BackgroundPick::new(0xFFFFFF, 0.6);
        assert_eq!(pick.hsv, [0.6, 0.0, 1.0]);
        // shown as it was: nothing to send, nothing to keep
        assert_eq!(pick.follow([0.6, 0.0, 1.0]), None);
        assert_eq!(pick.kept(), None);
        // each new colour sent once, the last kept
        assert_eq!(pick.follow([0.0, 1.0, 1.0]), Some(0xFF0000));
        assert_eq!(pick.follow([0.0, 1.0, 1.0]), None);
        assert_eq!(pick.follow([1.0 / 3.0, 1.0, 1.0]), Some(0x00FF00));
        assert_eq!(pick.kept(), Some(0x00FF00));
        // back where it began is still a change to keep (the live changes were sent)
        assert_eq!(pick.follow([0.6, 0.0, 1.0]), Some(0xFFFFFF));
        assert_eq!(pick.kept(), Some(0xFFFFFF));
    }
}
