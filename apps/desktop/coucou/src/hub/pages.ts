// Ported from Coucou (MIT, © 2026 Louis Raillé): windows/src/hub/pages.ts @ 0d7fe58884d99f275f3238e8dbe29ddee69fa9e8
// The Flow pages of the Coucou window, by id (shell.ts draws the Notch ones
// from the island's settings).

import type { FlowPageRenderer, PageId } from "./shell";
import { dictationPage } from "./pages/dictation";
import { notesPage } from "./pages/notes";
import { insightsPage } from "./pages/insights";
import { dictionaryPage, snippetsPage } from "./pages/words";
import { stylesPage } from "./pages/styles";
import { flowSettingsPage } from "./pages/flowsettings";
import { voicePage } from "./pages/voice";
import { mailBannedPage, mailOverviewPage, mailRulesPage, mailVIPPage } from "./pages/mail";

export const FLOW_PAGE_RENDERERS: Partial<Record<PageId, FlowPageRenderer>> = {
  dictation: dictationPage,
  notes: notesPage,
  insights: insightsPage,
  dictionary: dictionaryPage,
  snippets: snippetsPage,
  styles: stylesPage,
  flowsettings: flowSettingsPage,
  voice: voicePage,
  mailOverview: mailOverviewPage,
  mailRules: mailRulesPage,
  mailBanned: mailBannedPage,
  mailVIP: mailVIPPage,
};
