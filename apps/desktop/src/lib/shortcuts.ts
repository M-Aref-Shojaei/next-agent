// Keyboard policy. Q6 (docs/plan/Risks and Open Questions.md): Shift+Tab is
// reverse focus navigation, so mode cycling only claims it while the chat input
// is focused. A dedicated chord works from anywhere.

export interface KeyLike {
  key: string;
  shiftKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  metaKey: boolean;
}

/** Whether the key press should cycle the permission mode. */
export function shouldCycleMode(event: KeyLike, chatInputFocused: boolean): boolean {
  const mod = event.ctrlKey || event.metaKey;
  if (event.key === "Tab" && event.shiftKey && !mod && !event.altKey) return chatInputFocused;
  // Global chord: Ctrl/Cmd+Shift+M.
  return (
    (event.key === "M" || event.key === "m") && event.shiftKey && mod && !event.altKey
  );
}

/** Bypass needs the user to type the workspace name exactly. */
export function bypassConfirmed(typed: string, workspaceName: string): boolean {
  return workspaceName.length > 0 && typed.trim() === workspaceName;
}
