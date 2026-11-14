// Ported from Coucou (MIT, © 2026 Louis Raillé): NotchBuddy/Sources/App/IslandPlacement.swift @ 0d7fe58884d99f275f3238e8dbe29ddee69fa9e8
//
// Where the island lives: at the notch (top centre), or in a top corner as a
// small floating island under the menu bar. We port the three top placements
// (the bottom corners and the drag-to-snap of the character are not part of
// our overlay). AppKit coordinates (y up) unless noted. Pure, tested everywhere.

/// The macOS panel (`IslandWindowController.init`: `panelW = 720`, `panelH = 560`).
/// The island is drawn inside it; everything else in it is click-through.
pub const PANEL_W: f64 = 720.0;
pub const PANEL_H: f64 = 560.0;
/// The Windows/Linux panel (`island.rs` `PANEL_W`, `PANEL_H`).
pub const PC_PANEL_W: f64 = 720.0;
pub const PC_PANEL_H: f64 = 320.0;
/// The invisible strip that wakes a hidden island on a PC (`island.rs`).
pub const STRIP_W: f64 = 240.0;
pub const STRIP_H: f64 = 6.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Placement {
    #[default]
    Notch,
    TopLeft,
    TopRight,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    #[must_use]
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }
    #[must_use]
    pub fn max_x(&self) -> f64 {
        self.x + self.w
    }
    #[must_use]
    pub fn max_y(&self) -> f64 {
        self.y + self.h
    }
    #[must_use]
    pub fn mid_x(&self) -> f64 {
        self.x + self.w / 2.0
    }
}

/// A closed island never grows past the panel, and never shrinks below one
/// pixel (a zero-sized window can't be placed) (`island.rs` `clamp_collapsed`).
#[must_use]
pub fn clamp_collapsed((w, h): (f64, f64)) -> (f64, f64) {
    let ok = |v: f64, max: f64| {
        if v.is_finite() {
            v.clamp(1.0, max)
        } else {
            max
        }
    };
    (ok(w, PC_PANEL_W), ok(h, PC_PANEL_H))
}

impl Placement {
    /// Room the corner island keeps from the screen's edges (inside the visible frame).
    pub const MARGIN: f64 = 10.0;
    /// The island panel is wider than the open island; this much empty room on each side.
    pub const SIDE: f64 = 40.0;

    #[must_use]
    pub fn is_corner(self) -> bool {
        self != Self::Notch
    }

    /// The panel's origin on the screen. `screen` is the screen's frame, `visible` its
    /// visible frame (without the menu bar and the Dock).
    #[must_use]
    pub fn panel_origin(self, screen: Rect, visible: Rect, panel: (f64, f64)) -> (f64, f64) {
        let (m, s) = (Self::MARGIN, Self::SIDE);
        match self {
            Self::Notch => (screen.mid_x() - panel.0 / 2.0, screen.max_y() - panel.1),
            Self::TopLeft => (visible.x + m - s, visible.max_y() - m - panel.1),
            Self::TopRight => (
                visible.max_x() - m + s - panel.0,
                visible.max_y() - m - panel.1,
            ),
        }
    }

    /// The same with y going down (Windows, Linux): `work` is the work area, `top_inset`
    /// how far a top taskbar pushes the notch placement down (`island.rs` `top_inset`).
    #[must_use]
    pub fn panel_origin_top_down(
        self,
        screen: Rect,
        work: Rect,
        panel: (f64, f64),
        top_inset: f64,
    ) -> (f64, f64) {
        let (m, s) = (Self::MARGIN, Self::SIDE);
        match self {
            Self::Notch => (screen.mid_x() - panel.0 / 2.0, screen.y + top_inset),
            Self::TopLeft => (work.x + m - s, work.y + m),
            Self::TopRight => (work.max_x() - m + s - panel.0, work.y + m),
        }
    }

    /// The island's rect inside the panel for an island of `size`: centred at the top
    /// for the notch, held by its corner otherwise. Here y goes down from the panel's
    /// top (the page's coordinates); Coucou's version has y up.
    #[must_use]
    pub fn island_rect(self, panel: (f64, f64), size: (f64, f64)) -> Rect {
        let x = match self {
            Self::TopLeft => Self::SIDE,
            Self::TopRight => panel.0 - Self::SIDE - size.0,
            Self::Notch => (panel.0 - size.0) / 2.0,
        };
        Rect::new(x, 0.0, size.0, size.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PANEL: (f64, f64) = (PANEL_W, PANEL_H);

    #[test]
    fn notch_panel_is_centred_and_touches_the_top_edge() {
        let screen = Rect::new(0.0, 0.0, 1512.0, 982.0);
        let visible = Rect::new(0.0, 0.0, 1512.0, 950.0);
        let (x, y) = Placement::Notch.panel_origin(screen, visible, PANEL);
        assert_eq!(x, (1512.0 - 720.0) / 2.0);
        assert_eq!(y + PANEL_H, 982.0, "top edge on the screen's top edge");
    }

    #[test]
    fn notch_panel_follows_a_secondary_screen_origin() {
        let screen = Rect::new(-1920.0, 200.0, 1920.0, 1080.0);
        let (x, y) = Placement::Notch.panel_origin(screen, screen, PANEL);
        assert_eq!(x, -1920.0 + 600.0);
        assert_eq!(y, 200.0 + 1080.0 - 560.0);
    }

    #[test]
    fn corners_sit_under_the_menu_bar_with_a_margin() {
        let screen = Rect::new(0.0, 0.0, 1512.0, 982.0);
        let visible = Rect::new(0.0, 70.0, 1512.0, 880.0);
        let (lx, ly) = Placement::TopLeft.panel_origin(screen, visible, PANEL);
        assert_eq!((lx, ly), (10.0 - 40.0, 950.0 - 10.0 - 560.0));
        let (rx, _) = Placement::TopRight.panel_origin(screen, visible, PANEL);
        assert_eq!(rx, 1512.0 - 10.0 + 40.0 - 720.0);
        let island = Placement::TopLeft.island_rect(PANEL, (240.0, 24.0));
        assert_eq!(lx + island.x, 10.0, "the island itself is MARGIN in");
        let island = Placement::TopRight.island_rect(PANEL, (240.0, 24.0));
        assert_eq!(rx + island.max_x(), 1512.0 - 10.0);
    }

    #[test]
    fn top_down_respects_a_top_taskbar_for_the_notch_only() {
        let screen = Rect::new(0.0, 0.0, 1920.0, 1080.0);
        let work = Rect::new(0.0, 48.0, 1920.0, 1032.0);
        let pc = (PC_PANEL_W, PC_PANEL_H);
        assert_eq!(
            Placement::Notch.panel_origin_top_down(screen, work, pc, 48.0),
            (600.0, 48.0)
        );
        assert_eq!(
            Placement::TopRight.panel_origin_top_down(screen, work, pc, 48.0),
            (1920.0 - 10.0 + 40.0 - 720.0, 58.0)
        );
    }

    #[test]
    fn island_is_centred_for_the_notch() {
        assert_eq!(
            Placement::Notch.island_rect(PANEL, (344.0, 32.0)),
            Rect::new(188.0, 0.0, 344.0, 32.0)
        );
    }

    #[test]
    fn a_closed_island_stays_inside_the_panel() {
        assert_eq!(clamp_collapsed((160.0, 26.0)), (160.0, 26.0));
        assert_eq!(clamp_collapsed((5000.0, 900.0)), (PC_PANEL_W, PC_PANEL_H));
        assert_eq!(clamp_collapsed((0.0, -3.0)), (1.0, 1.0));
        assert_eq!(clamp_collapsed((f64::NAN, 26.0)), (PC_PANEL_W, 26.0));
    }
}
