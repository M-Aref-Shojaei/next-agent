// Next Agent: the Coucou window's entry since its settings moved into Next
// Agent's Settings (owner request: one design system). It is settings/main.ts's
// `main()` without the Notch pages: the window keeps Flow's pages that are not
// settings (dictation history, the Notetaker, statistics, dictionary, snippets,
// mail), and every settings page it links to opens in Next Agent's main window
// (`open_settings_window`, which Rust sends there: lib.rs `MOVED_PAGES`).
// settings/main.ts and its sections stay in the tree, unused, for re-syncs.

// The window's base styles came with settings/main.ts; the window looks as it did.
import "../settings/settings.css";
import { Bridge, onEvent } from "../core/bridge";
import { DEFAULT_SETTINGS, type Settings } from "../core/state";
import { startHub, type FlowPageRenderer, type PageId } from "./shell";
import { dictationPage } from "./pages/dictation";
import { notesPage } from "./pages/notes";
import { insightsPage } from "./pages/insights";
import { dictionaryPage, snippetsPage } from "./pages/words";
import { mailBannedPage, mailOverviewPage, mailRulesPage, mailVIPPage } from "./pages/mail";
import { isRtl, onLanguageChange, resolveLanguage, setLanguage, systemLanguages } from "../i18n/i18n";

/** The pages that live in Next Agent's Settings now (same list as lib.rs `MOVED_PAGES`). */
export const MOVED_PAGES: readonly PageId[] = [
  "general", "pills", "agents", "chat", "voice", "integrations", "phone", "shortcuts", "flowsettings", "styles",
];

const PAGES: Partial<Record<PageId, FlowPageRenderer>> = {
  dictation: dictationPage,
  notes: notesPage,
  insights: insightsPage,
  dictionary: dictionaryPage,
  snippets: snippetsPage,
  mailOverview: mailOverviewPage,
  mailRules: mailRulesPage,
  mailBanned: mailBannedPage,
  mailVIP: mailVIPPage,
};

let settings: Settings = { ...DEFAULT_SETTINGS };

function applyDirection() {
  document.documentElement.dir = isRtl() ? "rtl" : "ltr";
  document.title = "Next Agent";
}

async function main() {
  const boot = await Bridge.boot();
  if (boot) settings = { ...settings, ...boot.settings };
  setLanguage(resolveLanguage(settings.language, systemLanguages()));
  applyDirection();
  const hub = await startHub(document.getElementById("hub")!, {
    version: boot?.version ?? "",
    flowPages: PAGES,
    // No Notch page is drawn here any more: `elsewhere` takes them first.
    renderNotch: async () => {},
    elsewhere: (page) => {
      if (!MOVED_PAGES.includes(page)) return false;
      void Bridge.openSettingsWindow(page);
      return true;
    },
  });
  onLanguageChange(() => {
    applyDirection();
    hub.redraw();
  });
  void onEvent<Settings>("settings-changed", (s) => {
    settings = { ...settings, ...s };
    setLanguage(resolveLanguage(settings.language, systemLanguages()));
  });
}

void main();
