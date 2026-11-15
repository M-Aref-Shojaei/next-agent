// Ported from Coucou (MIT, © 2026 Louis Raillé): NotchBuddy/Sources/CoucouKit/IslandScreenGeometry.swift @ 0d7fe58884d99f275f3238e8dbe29ddee69fa9e8
//
// Resting island dimensions, using a physical notch only when the screen has
// one. Pure: compiled and tested on every platform.

/// Notch width used when the auxiliary areas cannot be read (points).
pub const FALLBACK_NOTCH_WIDTH: f64 = 184.0;
/// The resting bar on a screen without a notch (points).
pub const NO_NOTCH_WIDTH: f64 = 80.0;
/// The resting bar's height on a screen without a notch (points).
pub const NO_NOTCH_HEIGHT: f64 = 24.0;

/// Resting island size on one screen, in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IslandScreenGeometry {
    pub has_notch: bool,
    pub width: f64,
    pub height: f64,
}

impl IslandScreenGeometry {
    /// `safe_area_top > 0` means a notch. Its width is what the two
    /// auxiliary top areas (menu-bar room left and right of it) leave;
    /// `None` or a nonsense measurement falls back to 184 pt. Without a notch
    /// the island is a small 80 pt bar, at most as tall as the menu bar.
    #[must_use]
    pub fn new(
        screen_width: f64,
        safe_area_top: f64,
        auxiliary_left_width: Option<f64>,
        auxiliary_right_width: Option<f64>,
        menu_bar_height: f64,
    ) -> Self {
        let has_notch = safe_area_top > 0.0;
        if has_notch {
            let width = match (auxiliary_left_width, auxiliary_right_width) {
                (Some(left), Some(right)) => {
                    let measured = screen_width - left - right;
                    if measured > 0.0 && measured < screen_width {
                        measured
                    } else {
                        FALLBACK_NOTCH_WIDTH
                    }
                }
                _ => FALLBACK_NOTCH_WIDTH,
            };
            Self {
                has_notch,
                width,
                height: safe_area_top,
            }
        } else {
            Self {
                has_notch,
                width: NO_NOTCH_WIDTH,
                height: NO_NOTCH_HEIGHT.min(menu_bar_height),
            }
        }
    }
}

/// Menu-bar height of a screen: `frame.maxY − visibleFrame.maxY` while the
/// menu bar shows; when it auto-hides or an app is full screen that is zero,
/// so the status-bar thickness keeps a small resting bar
/// (`IslandWindowController.screenGeometry(for:)`).
#[must_use]
pub fn menu_bar_height(frame_max_y: f64, visible_max_y: f64, status_bar_thickness: f64) -> f64 {
    let visible = frame_max_y - visible_max_y;
    if visible > 0.0 {
        visible
    } else {
        status_bar_thickness
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notch_width_is_what_the_auxiliary_areas_leave() {
        // 14" MacBook Pro: 1512 pt wide, two 664 pt menu-bar halves.
        let g = IslandScreenGeometry::new(1512.0, 32.0, Some(664.0), Some(664.0), 32.0);
        assert_eq!(
            g,
            IslandScreenGeometry {
                has_notch: true,
                width: 184.0,
                height: 32.0
            }
        );
        let g = IslandScreenGeometry::new(1728.0, 38.0, Some(770.0), Some(770.0), 38.0);
        assert_eq!((g.width, g.height), (188.0, 38.0));
    }

    #[test]
    fn unreadable_or_nonsense_auxiliary_areas_fall_back() {
        let none = IslandScreenGeometry::new(1512.0, 32.0, None, Some(664.0), 32.0);
        assert_eq!(none.width, FALLBACK_NOTCH_WIDTH);
        // AppKit reports a zero rect when it has none: measured = whole width.
        let zero = IslandScreenGeometry::new(1512.0, 32.0, Some(0.0), Some(0.0), 32.0);
        assert_eq!(zero.width, FALLBACK_NOTCH_WIDTH);
        let negative = IslandScreenGeometry::new(1512.0, 32.0, Some(800.0), Some(800.0), 32.0);
        assert_eq!(negative.width, FALLBACK_NOTCH_WIDTH);
    }

    #[test]
    fn no_notch_is_a_small_bar_capped_by_the_menu_bar() {
        let g = IslandScreenGeometry::new(2560.0, 0.0, None, None, 25.0);
        assert_eq!(
            g,
            IslandScreenGeometry {
                has_notch: false,
                width: 80.0,
                height: 24.0
            }
        );
        let short = IslandScreenGeometry::new(2560.0, 0.0, None, None, 22.0);
        assert_eq!(short.height, 22.0);
    }

    #[test]
    fn hidden_menu_bar_uses_the_status_bar_thickness() {
        assert_eq!(menu_bar_height(982.0, 950.0, 24.0), 32.0);
        assert_eq!(menu_bar_height(982.0, 982.0, 24.0), 24.0);
    }
}
