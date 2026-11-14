// Ported from Coucou (MIT, © 2026 Louis Raillé): NotchBuddy/Sources/App/IslandDisplayChoice.swift @ 0d7fe58884d99f275f3238e8dbe29ddee69fa9e8
//
// Which screen hosts the island. Pure: the macOS glue feeds it `NSScreen`
// candidates, the Windows/Linux runtime feeds it Tauri monitors.

/// Persisted as [`DisplayChoice::storage_value`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum DisplayChoice {
    /// Default: the built-in screen with a notch, else the main screen.
    #[default]
    Notch,
    /// The screen that carries the menu bar (first in `NSScreen.screens`).
    MenuBar,
    /// The island moves to the screen under the cursor while it is not open.
    FollowMouse,
    /// A specific screen, by a stable id: the display UUID on macOS (stable
    /// across reboots), the `at:x,y|name|wxh` key on Windows and Linux.
    Display(String),
}

const DISPLAY_PREFIX: &str = "display:";

impl DisplayChoice {
    /// Unknown values (and an empty id) fall back to [`DisplayChoice::Notch`].
    #[must_use]
    pub fn from_storage(value: &str) -> Self {
        match value {
            "menuBar" => Self::MenuBar,
            "followMouse" => Self::FollowMouse,
            _ => match value.strip_prefix(DISPLAY_PREFIX) {
                Some(id) if !id.is_empty() => Self::Display(id.to_string()),
                _ => Self::Notch,
            },
        }
    }

    #[must_use]
    pub fn storage_value(&self) -> String {
        match self {
            Self::Notch => "notch".into(),
            Self::MenuBar => "menuBar".into(),
            Self::FollowMouse => "followMouse".into(),
            Self::Display(id) => format!("{DISPLAY_PREFIX}{id}"),
        }
    }
}

/// What the resolver needs to know about a connected screen, in
/// `NSScreen.screens` order (the menu-bar screen first).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayCandidate {
    pub id: Option<String>,
    pub has_notch: bool,
    pub contains_mouse: bool,
}

/// Index of the screen that should host the island, or `None` for the
/// default (notch screen, else main screen). A disconnected chosen screen
/// falls back too.
#[must_use]
pub fn resolve(choice: &DisplayChoice, candidates: &[DisplayCandidate]) -> Option<usize> {
    if candidates.is_empty() {
        return None;
    }
    let notch = candidates.iter().position(|c| c.has_notch);
    match choice {
        DisplayChoice::Notch => notch,
        DisplayChoice::MenuBar => Some(0),
        DisplayChoice::FollowMouse => candidates.iter().position(|c| c.contains_mouse).or(notch),
        DisplayChoice::Display(id) => candidates
            .iter()
            .position(|c| c.id.as_deref() == Some(id.as_str()))
            .or(notch),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(id: &str, has_notch: bool, contains_mouse: bool) -> DisplayCandidate {
        DisplayCandidate {
            id: Some(id.into()),
            has_notch,
            contains_mouse,
        }
    }

    #[test]
    fn storage_round_trips_and_unknown_is_notch() {
        for choice in [
            DisplayChoice::Notch,
            DisplayChoice::MenuBar,
            DisplayChoice::FollowMouse,
            DisplayChoice::Display("37D8832A-2D66-02CA-B9F7-8F30A301B230".into()),
        ] {
            assert_eq!(DisplayChoice::from_storage(&choice.storage_value()), choice);
        }
        assert_eq!(
            DisplayChoice::from_storage("display:"),
            DisplayChoice::Notch
        );
        assert_eq!(DisplayChoice::from_storage("garbage"), DisplayChoice::Notch);
    }

    #[test]
    fn resolver_follows_the_choice_then_falls_back_to_the_notch() {
        // External display first (it carries the menu bar), the laptop second.
        let screens = [c("EXT", false, false), c("LAPTOP", true, true)];
        assert_eq!(resolve(&DisplayChoice::Notch, &screens), Some(1));
        assert_eq!(resolve(&DisplayChoice::MenuBar, &screens), Some(0));
        assert_eq!(resolve(&DisplayChoice::FollowMouse, &screens), Some(1));
        assert_eq!(
            resolve(&DisplayChoice::Display("EXT".into()), &screens),
            Some(0)
        );
        // Unplugged: back to the notch screen.
        assert_eq!(
            resolve(&DisplayChoice::Display("GONE".into()), &screens),
            Some(1)
        );
    }

    #[test]
    fn no_notch_anywhere_means_the_default_screen() {
        let screens = [c("A", false, false), c("B", false, false)];
        assert_eq!(resolve(&DisplayChoice::Notch, &screens), None);
        assert_eq!(resolve(&DisplayChoice::FollowMouse, &screens), None);
        assert_eq!(resolve(&DisplayChoice::Notch, &[]), None);
    }
}
