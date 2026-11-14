// Small glyphs for the island. Coucou uses SF Symbols, which we cannot ship;
// these are the closest Lucide icons (lucide-static 0.460.0, ISC licence,
// © Lucide Contributors and Cole Bemis; see NOTICE.md). 24×24, stroked.
//
//   SF Symbol (Coucou)     Lucide
//   house.fill             house
//   gearshape              settings
//   xmark                  x
//   chevron.up             chevron-up

export interface Glyph {
  paths: string[];
  circles?: { cx: number; cy: number; r: number }[];
}

export const GLYPHS = {
  house: {
    paths: [
      "M15 21v-8a1 1 0 0 0-1-1h-4a1 1 0 0 0-1 1v8",
      "M3 10a2 2 0 0 1 .709-1.528l7-5.999a2 2 0 0 1 2.582 0l7 5.999A2 2 0 0 1 21 10v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z",
    ],
  },
  gear: {
    paths: [
      "M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z",
    ],
    circles: [{ cx: 12, cy: 12, r: 3 }],
  },
  xmark: { paths: ["M18 6 6 18", "m6 6 12 12"] },
  chevronUp: { paths: ["m18 15-6-6-6 6"] },
} satisfies Record<string, Glyph>;
