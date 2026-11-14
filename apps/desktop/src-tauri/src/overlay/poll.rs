// Ported from Coucou (MIT, © 2026 Louis Raillé): NotchBuddy/Sources/App/IslandWindowController.swift (pollFrame, adjustPollRate, observeScreenForPolling) and windows/src-tauri/src/island.rs (HIT_MARGIN, spawn_cursor_poll) @ 0d7fe58884d99f275f3238e8dbe29ddee69fa9e8
//
// Click-through state machine. The panel ignores the mouse everywhere except
// over the island; a cursor poll decides, and only calls into the OS when the
// answer changes. The poll is adaptive: 60 Hz while the island is open or the
// pointer is close to it, 20 Hz in the band around the panel, 8 Hz elsewhere,
// and paused while the display sleeps. Pure: the thread lives in `mod.rs`.

use next_agent_macos_panel::Rect;
use std::time::Duration;

/// 60 Hz: open, or the pointer close to the island.
pub const FAST_POLL: Duration = Duration::from_micros(16_667);
/// 20 Hz: in the wide band around the panel (a pointer flicked up still
/// reaches the close zone within one tick).
pub const NEAR_POLL: Duration = Duration::from_millis(50);
/// 8 Hz: the pointer is elsewhere.
pub const IDLE_POLL: Duration = Duration::from_millis(125);
/// Display asleep: only check, once a second, whether it woke up.
pub const ASLEEP_POLL: Duration = Duration::from_secs(1);

/// Margin around the island that still counts as on it (logical px): 6 pt
/// on the Mac, 14 px on a PC, where a click must never be swallowed.
pub const HIT_MARGIN: f64 = if cfg!(target_os = "macos") { 6.0 } else { 14.0 };
/// "Close to the island": the island grown by this much (x, y).
const NEAR_ISLAND: (f64, f64) = (200.0, 160.0);
/// The band around the whole panel.
const BAND: f64 = 120.0;

fn grow(r: Rect, dx: f64, dy: f64) -> Rect {
    Rect::new(r.x - dx, r.y - dy, r.w + 2.0 * dx, r.h + 2.0 * dy)
}

fn contains(r: Rect, (x, y): (f64, f64)) -> bool {
    x >= r.x && x <= r.max_x() && y >= r.y && y <= r.max_y()
}

/// One poll's view of the world. Coordinates are logical px relative to the
/// panel's top-left corner.
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub struct PollInput {
    pub cursor: Option<(f64, f64)>,
    /// The island as the page draws it (`None` until it reports one).
    pub island: Option<Rect>,
    pub panel: (f64, f64),
    pub expanded: bool,
    /// A resting island on a screen without a notch takes no margin: it
    /// must not swallow clicks meant for the app window just below it.
    pub bar_without_notch: bool,
    pub display_asleep: bool,
}

/// Result of one poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tick {
    /// `Some(inside)` only when the answer changed: the window takes the
    /// mouse exactly while inside.
    pub changed: Option<bool>,
    pub next_poll: Duration,
}

#[derive(Debug, Default)]
pub struct ClickThrough {
    inside: Option<bool>,
}

impl ClickThrough {
    pub fn tick(&mut self, input: &PollInput) -> Tick {
        if input.display_asleep {
            return self.settle(false, ASLEEP_POLL);
        }
        let Some((cursor, island)) = input.cursor.zip(input.island) else {
            return self.settle(false, IDLE_POLL);
        };
        let margin = if input.bar_without_notch && !input.expanded {
            0.0
        } else {
            HIT_MARGIN
        };
        let inside = contains(grow(island, margin, margin), cursor);
        let near = contains(grow(island, NEAR_ISLAND.0, NEAR_ISLAND.1), cursor);
        let panel = Rect::new(0.0, 0.0, input.panel.0, input.panel.1);
        let in_band = contains(grow(panel, BAND, BAND), cursor);
        let next = if input.expanded || near {
            FAST_POLL
        } else if in_band {
            NEAR_POLL
        } else {
            IDLE_POLL
        };
        self.settle(inside, next)
    }

    fn settle(&mut self, inside: bool, next_poll: Duration) -> Tick {
        let changed = (self.inside != Some(inside)).then_some(inside);
        self.inside = Some(inside);
        Tick { changed, next_poll }
    }

    /// Forget the last answer so the next tick re-applies it (after the
    /// window was re-created or re-placed).
    pub fn reset(&mut self) {
        self.inside = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(cursor: (f64, f64)) -> PollInput {
        PollInput {
            cursor: Some(cursor),
            // A 344×32 compact island centred in a 720×320 panel.
            island: Some(Rect::new(188.0, 0.0, 344.0, 32.0)),
            panel: (720.0, 320.0),
            expanded: false,
            bar_without_notch: false,
            display_asleep: false,
        }
    }

    #[test]
    fn toggles_only_when_the_answer_changes() {
        let mut ct = ClickThrough::default();
        // First poll always reports (far: click-through).
        assert_eq!(ct.tick(&input((900.0, 900.0))).changed, Some(false));
        assert_eq!(ct.tick(&input((901.0, 900.0))).changed, None);
        assert_eq!(ct.tick(&input((300.0, 10.0))).changed, Some(true));
        assert_eq!(ct.tick(&input((301.0, 11.0))).changed, None);
        // Inside the hit margin still counts; beyond it, released.
        let edge = 532.0 + HIT_MARGIN - 1.0;
        assert_eq!(ct.tick(&input((edge, 10.0))).changed, None);
        assert_eq!(
            ct.tick(&input((532.0 + HIT_MARGIN + 1.0, 10.0))).changed,
            Some(false)
        );
    }

    #[test]
    fn polls_fast_near_or_open_slower_in_the_band_slowest_elsewhere() {
        let mut ct = ClickThrough::default();
        assert_eq!(ct.tick(&input((300.0, 100.0))).next_poll, FAST_POLL);
        assert_eq!(ct.tick(&input((760.0, 300.0))).next_poll, NEAR_POLL);
        assert_eq!(ct.tick(&input((2000.0, 900.0))).next_poll, IDLE_POLL);
        let open = PollInput {
            expanded: true,
            ..input((2000.0, 900.0))
        };
        assert_eq!(ct.tick(&open).next_poll, FAST_POLL);
        let unknown = PollInput {
            cursor: None,
            ..input((0.0, 0.0))
        };
        assert_eq!(ct.tick(&unknown).next_poll, IDLE_POLL);
    }

    #[test]
    fn pauses_and_lets_clicks_through_while_the_display_sleeps() {
        let mut ct = ClickThrough::default();
        assert_eq!(ct.tick(&input((300.0, 10.0))).changed, Some(true));
        let asleep = PollInput {
            display_asleep: true,
            ..input((300.0, 10.0))
        };
        assert_eq!(
            ct.tick(&asleep),
            Tick {
                changed: Some(false),
                next_poll: ASLEEP_POLL
            }
        );
        assert_eq!(ct.tick(&asleep).changed, None);
        // Awake again: back to the normal answer.
        assert_eq!(ct.tick(&input((300.0, 10.0))).changed, Some(true));
    }

    #[test]
    fn a_resting_bar_without_notch_takes_no_margin() {
        let mut ct = ClickThrough::default();
        let bar = PollInput {
            bar_without_notch: true,
            ..input((300.0, 32.0 + HIT_MARGIN - 1.0))
        };
        assert_eq!(ct.tick(&bar).changed, Some(false));
        let open = PollInput {
            expanded: true,
            ..bar
        };
        assert_eq!(ct.tick(&open).changed, Some(true));
    }

    #[test]
    fn no_island_means_click_through_and_reset_re_applies() {
        let mut ct = ClickThrough::default();
        let none = PollInput {
            island: None,
            ..input((300.0, 10.0))
        };
        assert_eq!(ct.tick(&none).changed, Some(false));
        assert_eq!(ct.tick(&none).changed, None);
        ct.reset();
        assert_eq!(ct.tick(&none).changed, Some(false), "reset re-applies");
    }
}
