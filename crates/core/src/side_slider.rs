//! The side bar's size and opacity sliders: what a tap on one does, and how far a drag moves its
//! thumb. Values run from 0 at the bottom to 1 at the top.

/// Within this far of a slider's middle line (points) a drag moves the thumb with the finger.
const FULL_REACH: f32 = 52.0;

/// The least share of the finger's move a drag passes on, however far away the finger is.
const FINEST: f32 = 0.01;

/// The share of the finger's move along a slider that moves its thumb, with the finger `across`
/// points from the slider's middle line: all of it close by, then less with the square of the
/// distance (a quarter at twice [`FULL_REACH`]), never under [`FINEST`]. Dragging out sideways
/// first gives finer control.
#[uniffi::export]
pub fn side_slider_drag_share(across: f32) -> f32 {
    let d = across.abs();
    if !d.is_finite() {
        return FINEST;
    }
    if d <= FULL_REACH {
        return 1.0;
    }
    (FULL_REACH / d).powi(2).max(FINEST)
}

/// What a tap on a side bar slider does.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Enum)]
pub enum SideSliderTap {
    /// On the thumb: the slider's panel opens and stays up.
    Panel,
    /// Elsewhere on the track: the thumb goes to this value.
    Jump { value: f32 },
}

/// A tap at `at` (the value under the finger, the thumb's middle there) on a slider at `value`,
/// whose thumb reaches `thumb` (in value units) either side of its middle: on the thumb it opens
/// the panel; elsewhere the thumb goes to the tap, or onto a saved notch the tap is as close to.
#[uniffi::export]
pub fn side_slider_tap(at: f32, value: f32, thumb: f32, notches: Vec<f32>) -> SideSliderTap {
    if (at - value).abs() <= thumb {
        return SideSliderTap::Panel;
    }
    let near = notches
        .into_iter()
        .filter(|n| (n - at).abs() <= thumb)
        .min_by(|a, b| (a - at).abs().total_cmp(&(b - at).abs()));
    SideSliderTap::Jump { value: near.unwrap_or(at).clamp(0.0, 1.0) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drag_off_to_the_side_moves_the_thumb_less_as_measured() {
        // measured: the share of a 60 point move along, by the finger's distance from the slider
        for (across, measured) in
            [(30.0, 1.0), (45.0, 1.0), (60.0, 0.76), (90.0, 0.36), (120.0, 0.19), (150.0, 0.117), (180.0, 0.083)]
        {
            let share = side_slider_drag_share(across);
            assert!((share - measured).abs() < 0.03, "{across}: {share} against {measured}");
        }
        // either side, and never stuck
        assert_eq!(side_slider_drag_share(-30.0), 1.0);
        assert_eq!(side_slider_drag_share(5000.0), FINEST);
        assert_eq!(side_slider_drag_share(f32::NAN), FINEST);
    }

    #[test]
    fn a_tap_on_the_thumb_opens_the_panel_and_elsewhere_the_thumb_goes_there() {
        assert_eq!(side_slider_tap(0.52, 0.5, 0.06, vec![]), SideSliderTap::Panel);
        assert_eq!(side_slider_tap(0.2, 0.5, 0.06, vec![]), SideSliderTap::Jump { value: 0.2 });
        // onto a notch the tap is close to, the nearest of two
        assert_eq!(side_slider_tap(0.21, 0.5, 0.06, vec![0.25, 0.18]), SideSliderTap::Jump { value: 0.18 });
        assert_eq!(side_slider_tap(0.9, 0.5, 0.06, vec![0.25]), SideSliderTap::Jump { value: 0.9 });
        // past the ends it holds to them
        assert_eq!(side_slider_tap(1.02, 0.5, 0.01, vec![]), SideSliderTap::Jump { value: 1.0 });
    }
}
