// Ported from Coucou (MIT, © 2026 Louis Raillé): windows/src-tauri/src/island.rs @ 0d7fe58884d99f275f3238e8dbe29ddee69fa9e8
//
// Windows and Linux placement: which monitor (a stable display key, the
// cursor's monitor, the primary), how far a top taskbar pushes the island
// down, and the panel rectangle in physical pixels. Pure; the window calls
// live in `mod.rs`. macOS places the panel natively (`next_agent_macos_panel`).

use next_agent_macos_panel::{DisplayChoice, Placement, Rect};

/// Resting island on a PC (no notch): `layout.ts` `NOTCH_W`, `NOTCH_H`; the
/// compact island is 104 px wider (`COMPACT_W = NOTCH_W + 104`).
pub const PC_REST_W: f64 = 184.0;
pub const PC_REST_H: f64 = 32.0;
pub const PC_COMPACT_EXTRA: f64 = 104.0;

/// One display in physical pixels, as Tauri reports it.
#[derive(Debug, Clone, PartialEq)]
pub struct MonitorGeom {
    pub name: Option<String>,
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub work_x: i32,
    pub work_y: i32,
    pub work_w: u32,
    pub work_h: u32,
    pub scale: f64,
}

impl MonitorGeom {
    fn contains(&self, (cx, cy): (f64, f64)) -> bool {
        cx >= f64::from(self.x)
            && cx < f64::from(self.x) + f64::from(self.w)
            && cy >= f64::from(self.y)
            && cy < f64::from(self.y) + f64::from(self.h)
    }
}

#[allow(clippy::cast_possible_truncation)]
fn round_i32(v: f64) -> i32 {
    v.round() as i32
}

/// What a display is remembered by: its logical origin, plus its name and
/// logical size, so it is still found after the layout is rearranged or the
/// resolution changes (the Mac keeps the display's UUID for the same reason;
/// Tauri has no stable id).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayId {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl DisplayId {
    /// `at:<x>,<y>` stays first, so a preference saved before still matches.
    #[must_use]
    pub fn key(&self) -> String {
        format!(
            "at:{},{}|{}|{}x{}",
            self.x,
            self.y,
            self.name.replace('|', " "),
            self.w,
            self.h
        )
    }

    #[must_use]
    pub fn label(&self) -> String {
        format!(
            "{} — {}×{} at {},{}",
            self.name, self.w, self.h, self.x, self.y
        )
    }
}

#[must_use]
pub fn describe(m: &MonitorGeom) -> DisplayId {
    let s = m.scale;
    DisplayId {
        name: m.name.clone().unwrap_or_else(|| "Display".into()),
        x: round_i32(f64::from(m.x) / s),
        y: round_i32(f64::from(m.y) / s),
        w: round_i32(f64::from(m.w) / s),
        h: round_i32(f64::from(m.h) / s),
    }
}

/// Which display a saved `at:` key points at, best match first: same place
/// and name; the same name and size elsewhere (layout rearranged); the same
/// name alone when unique (resolution changed); the same place. `None` means
/// unplugged, and the caller falls back to the primary display.
#[must_use]
pub fn pick_display(pref: &str, displays: &[DisplayId]) -> Option<usize> {
    let rest = pref.strip_prefix("at:")?;
    let mut parts = rest.split('|');
    let (x, y) = parts.next()?.split_once(',')?;
    let (x, y) = (x.trim().parse::<i32>().ok()?, y.trim().parse::<i32>().ok()?);
    let name = parts.next();
    let size = parts.next().and_then(|s| {
        let (w, h) = s.split_once('x')?;
        Some((w.parse::<i32>().ok()?, h.parse::<i32>().ok()?))
    });
    let at = |d: &DisplayId| d.x == x && d.y == y;
    if let Some(name) = name {
        if let Some(i) = displays.iter().position(|d| at(d) && d.name == name) {
            return Some(i);
        }
        if let Some((w, h)) = size {
            if let Some(i) = displays
                .iter()
                .position(|d| d.name == name && d.w == w && d.h == h)
            {
                return Some(i);
            }
        }
        let mut same_name = displays.iter().enumerate().filter(|(_, d)| d.name == name);
        if let (Some((i, _)), None) = (same_name.next(), same_name.next()) {
            return Some(i);
        }
    }
    displays.iter().position(at)
}

/// The monitor the island lives on: the chosen one, the one under the cursor
/// (follow the mouse), else the primary, else the first.
#[must_use]
pub fn target_monitor(
    monitors: &[MonitorGeom],
    choice: &DisplayChoice,
    cursor: Option<(f64, f64)>,
    primary: Option<usize>,
) -> Option<usize> {
    if monitors.is_empty() {
        return None;
    }
    match choice {
        DisplayChoice::Display(key) => {
            let ids: Vec<DisplayId> = monitors.iter().map(describe).collect();
            if let Some(i) = pick_display(key, &ids) {
                return Some(i);
            }
        }
        DisplayChoice::FollowMouse => {
            if let Some(i) = cursor.and_then(|c| monitors.iter().position(|m| m.contains(c))) {
                return Some(i);
            }
        }
        DisplayChoice::Notch | DisplayChoice::MenuBar => {}
    }
    primary.filter(|&i| i < monitors.len()).or(Some(0))
}

/// Windows: how far a taskbar docked at the top of this display (or another
/// app bar) pushes the island down, read from the work area. An auto-hidden
/// taskbar takes no work area and moves nothing; nonsense is ignored.
#[must_use]
pub fn top_inset_from(monitor_y: i32, work_y: i32) -> i32 {
    let inset = work_y - monitor_y;
    if (0..200).contains(&inset) {
        inset
    } else {
        0
    }
}

/// Window rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placed {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn px(logical: f64, scale: f64) -> u32 {
    (logical * scale).round().max(1.0) as u32
}

/// The panel on `m` for `placement`. `use_work_area` (Windows) keeps the
/// notch placement below a top taskbar; Linux anchors to the screen edge.
#[must_use]
pub fn place(
    m: &MonitorGeom,
    placement: Placement,
    size: (f64, f64),
    use_work_area: bool,
) -> Placed {
    let s = m.scale;
    let logical = |x: i32, y: i32, w: u32, h: u32| {
        Rect::new(
            f64::from(x) / s,
            f64::from(y) / s,
            f64::from(w) / s,
            f64::from(h) / s,
        )
    };
    let screen = logical(m.x, m.y, m.w, m.h);
    let work = logical(m.work_x, m.work_y, m.work_w, m.work_h);
    let inset = if use_work_area {
        f64::from(top_inset_from(m.y, m.work_y)) / s
    } else {
        0.0
    };
    let (x, y) = placement.panel_origin_top_down(screen, work, size, inset);
    Placed {
        x: round_i32(x * s),
        y: round_i32(y * s),
        w: px(size.0, s),
        h: px(size.1, s),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mon(name: &str, x: i32, y: i32, w: u32, h: u32, scale: f64) -> MonitorGeom {
        MonitorGeom {
            name: Some(name.into()),
            x,
            y,
            w,
            h,
            work_x: x,
            work_y: y + 48,
            work_w: w,
            work_h: h - 48,
            scale,
        }
    }

    fn d(name: &str, x: i32, y: i32, w: i32, h: i32) -> DisplayId {
        DisplayId {
            name: name.into(),
            x,
            y,
            w,
            h,
        }
    }

    #[test]
    fn a_display_is_found_again_after_changes() {
        let dell = d("DELL U2720Q", 1920, 0, 2560, 1440);
        let lap = d("eDP-1", 0, 0, 1920, 1200);
        let key = dell.key();
        assert_eq!(pick_display(&key, &[lap.clone(), dell.clone()]), Some(1));
        // Rearranged: the Dell moved to the left of the laptop.
        let moved = [
            d("eDP-1", 2560, 0, 1920, 1200),
            d("DELL U2720Q", 0, 0, 2560, 1440),
        ];
        assert_eq!(pick_display(&key, &moved), Some(1));
        // Resolution changed, still the only Dell.
        let rescaled = [lap.clone(), d("DELL U2720Q", 1920, 0, 1920, 1080)];
        assert_eq!(pick_display(&key, &rescaled), Some(1));
        // Unplugged: nothing, so the caller falls back to the primary display.
        assert_eq!(pick_display(&key, &[lap]), None);
    }

    #[test]
    fn two_identical_monitors_are_told_apart_by_place() {
        let a = d("LG 27UL500", 0, 0, 1920, 1080);
        let b = d("LG 27UL500", 1920, 0, 1920, 1080);
        assert_eq!(pick_display(&b.key(), &[a.clone(), b.clone()]), Some(1));
        assert_eq!(pick_display(&a.key(), &[a, b]), Some(0));
        assert_eq!(pick_display("at:nonsense", &[]), None);
    }

    #[test]
    fn monitor_choice_key_then_cursor_then_primary() {
        let ms = [
            mon("A", 0, 0, 1920, 1080, 1.0),
            mon("B", 1920, 0, 1920, 1080, 1.0),
        ];
        let b = DisplayChoice::Display(describe(&ms[1]).key());
        assert_eq!(
            target_monitor(&ms, &b, Some((10.0, 10.0)), Some(0)),
            Some(1)
        );
        let follow = DisplayChoice::FollowMouse;
        assert_eq!(
            target_monitor(&ms, &follow, Some((2000.0, 5.0)), Some(0)),
            Some(1)
        );
        assert_eq!(target_monitor(&ms, &follow, None, Some(0)), Some(0));
        let gone = DisplayChoice::Display("at:9999,0|X|1x1".into());
        assert_eq!(target_monitor(&ms, &gone, None, Some(1)), Some(1));
        assert_eq!(
            target_monitor(&ms, &DisplayChoice::Notch, None, None),
            Some(0)
        );
        assert_eq!(target_monitor(&[], &DisplayChoice::Notch, None, None), None);
    }

    #[test]
    fn a_top_taskbar_moves_the_island_down_and_nothing_else_does() {
        assert_eq!(top_inset_from(0, 0), 0);
        assert_eq!(top_inset_from(-1080, -1032), 48);
        assert_eq!(top_inset_from(0, -20), 0);
        assert_eq!(top_inset_from(0, 900), 0);
    }

    #[test]
    fn notch_placement_is_centred_and_scaled() {
        let m = mon("A", 0, 0, 1920, 1080, 1.0);
        // Compact PC island: 288×32.
        assert_eq!(
            place(&m, Placement::Notch, (288.0, 32.0), false),
            Placed {
                x: 816,
                y: 0,
                w: 288,
                h: 32
            }
        );
        // Windows: below the 48 px top taskbar of this test monitor.
        assert_eq!(place(&m, Placement::Notch, (288.0, 32.0), true).y, 48);
        let hi = mon("B", 1920, -200, 3000, 2000, 2.0);
        let p = place(&hi, Placement::Notch, (640.0, 160.0), false);
        assert_eq!((p.w, p.h), (1280, 320));
        assert_eq!(p.x, 1920 + (3000 - 1280) / 2);
        assert_eq!(p.y, -200);
    }

    #[test]
    fn corner_placements_keep_the_margin_inside_the_work_area() {
        let m = mon("A", 0, 0, 1920, 1080, 1.0);
        let left = place(&m, Placement::TopLeft, (288.0, 32.0), true);
        assert_eq!((left.x, left.y), (10 - 40, 48 + 10));
        let right = place(&m, Placement::TopRight, (288.0, 32.0), true);
        assert_eq!(right.x + 288, 1920 - 10 + 40);
    }
}
