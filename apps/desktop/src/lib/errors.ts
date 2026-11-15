export function errorText(e: unknown): string {
  return String(e instanceof Error ? e.message : e);
}
