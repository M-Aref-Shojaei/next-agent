import type { ModeName } from "@next-agent/contracts";

/** `Shift+Tab` order. Bypass is never reachable by cycling. */
export const EVERYDAY_MODES: readonly ModeName[] = ["plan", "manual", "auto"];

export function cycleMode(current: ModeName): ModeName {
  const index = EVERYDAY_MODES.indexOf(current);
  if (index === -1) return "manual";
  return EVERYDAY_MODES[(index + 1) % EVERYDAY_MODES.length];
}

export const MODE_LABELS: Record<ModeName, string> = {
  plan: "Plan",
  manual: "Manual",
  auto: "Auto",
  bypassPermissions: "Bypass",
};

export const MODE_TONE: Record<ModeName, "info" | "neutral" | "warn" | "danger"> = {
  plan: "info",
  manual: "neutral",
  auto: "warn",
  bypassPermissions: "danger",
};
