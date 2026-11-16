<script lang="ts">
  import { onMount } from "svelte";
  import { api, type LogEntry, type LogLevel } from "$lib/api.ts";
  import { errorText } from "$lib/errors.ts";
  import {
    Badge, Banner, Button, Select, SegmentedControl, SettingsLayout, SettingsRow, SettingsSection, Table,
    type Option, type SettingsNavItem, type TableColumn, type Tone,
  } from "$lib/ds/index.ts";

  const LEVELS: Option<LogLevel>[] = [
    { value: "error", label: "Error", description: "Only failures." },
    { value: "warn", label: "Warn", description: "Failures and things that went wrong but recovered." },
    { value: "info", label: "Info", description: "The default: what happened, once per real event." },
    { value: "debug", label: "Debug", description: "Everything, including noisy detail." },
  ];
  // The areas of the plan (a log event's `target`).
  const AREAS = [
    "app", "router", "agent", "broker", "tasks", "store", "worktree", "vault", "notes", "boards", "design",
    "channels", "learning", "cron", "overlay", "upstreams", "updates", "ui",
  ];
  const levelFilter: Option<string>[] = [{ value: "all", label: "All levels" }, ...LEVELS.map(({ value, label }) => ({ value, label }))];
  const areaFilter: Option<string>[] = [{ value: "all", label: "All areas" }, ...AREAS.map((a) => ({ value: a, label: a }))];
  const TONE: Record<LogLevel, Tone> = { error: "danger", warn: "warn", info: "neutral", debug: "neutral" };

  const nav: SettingsNavItem[] = [
    { id: "level", label: "Log level", href: "#level" },
    { id: "entries", label: "Recent entries", href: "#entries" },
    { id: "export", label: "Files", href: "#export" },
  ];

  let level = $state<LogLevel>("info");
  let levelPick = $state<string>("all");
  let areaPick = $state<string>("all");
  let entries = $state<LogEntry[]>([]);
  let message = $state("");
  let exported = $state<{ path: string; entries: number } | null>(null);
  let exporting = $state(false);

  async function refresh() {
    try {
      entries = await api.diagRecent({
        level: levelPick === "all" ? undefined : (levelPick as LogLevel),
        target: areaPick === "all" ? undefined : areaPick,
        limit: 200,
      });
    } catch (e) {
      message = errorText(e);
    }
  }

  async function setLevel(next: LogLevel) {
    try {
      await api.diagSetLevel(next);
      level = next;
    } catch (e) {
      message = errorText(e);
    }
  }

  async function openFolder() {
    try {
      await api.diagOpenFolder();
    } catch (e) {
      message = errorText(e);
    }
  }

  async function exportLog() {
    exporting = true;
    try {
      exported = await api.diagExport();
    } catch (e) {
      message = errorText(e);
    } finally {
      exporting = false;
    }
  }

  onMount(() => {
    void api.settingGet("logging.level").then((v) => {
      if (v && LEVELS.some((l) => l.value === v)) level = v as LogLevel;
    }).catch(() => {});
    const timer = setInterval(() => void refresh(), 5000);
    return () => clearInterval(timer);
  });

  // Filters re-query at once instead of waiting for the next tick.
  $effect(() => {
    levelPick; areaPick;
    void refresh();
  });

  const time = (ts: string) => new Date(ts).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
  const detail = (e: LogEntry) =>
    Object.entries(e.fields).map(([k, v]) => `${k}=${String(v)}`).join("  ");

  const columns: TableColumn<LogEntry>[] = [
    { key: "ts", label: "Time", nowrap: true, value: (e) => time(e.ts), sortValue: (e) => e.ts },
    { key: "level", label: "Level", value: (e) => e.level },
    { key: "target", label: "Area", value: (e) => e.target },
    { key: "event", label: "Event", value: (e) => e.event },
    { key: "fields", label: "Details", value: detail },
  ];
</script>

<SettingsLayout
  title="Diagnostics"
  description="A local log of how the app behaves: what started, what failed and how long things took. It stays on this computer and is never sent anywhere."
  {nav}
>
  {#snippet top()}<a href="/settings">← Back to settings</a>{/snippet}

  {#if level === "debug"}
    <Banner tone="warn" title="Debug logging is on">
      The log is noisier and grows faster. Switch back to Info when you are done.
    </Banner>
  {/if}
  {#if message}<Banner tone="danger" ondismiss={() => (message = "")}>{message}</Banner>{/if}

  <SettingsSection
    id="level"
    title="Log level"
    description="Changes apply at once and are remembered."
    {footer}
  >
    <SettingsRow label="Level" description="How much detail is written.">
      {#snippet children(ctl)}
        <SegmentedControl labelledby={ctl.labelId} options={LEVELS} value={level} onchange={setLevel} />
      {/snippet}
    </SettingsRow>
  </SettingsSection>

  <SettingsSection id="entries" title="Recent entries" description="Newest first. Refreshes every 5 seconds while this page is open.">
    {#snippet actions()}
      <Select size="sm" aria-label="Filter by level" options={levelFilter} bind:value={levelPick} />
      <Select size="sm" aria-label="Filter by area" options={areaFilter} bind:value={areaPick} />
    {/snippet}
    <Table
      label="Recent log entries"
      {columns}
      rows={entries}
      rowKey={(e, i) => `${e.ts}-${i}`}
      empty="No entries"
      emptyDescription="Nothing matches these filters yet."
    >
      {#snippet cell(row, col)}
        {#if col.key === "level"}
          <Badge tone={TONE[row.level]}>{row.level}</Badge>
        {:else if col.key === "fields"}
          <code>{detail(row)}</code>
        {:else}
          {col.value?.(row) ?? ""}
        {/if}
      {/snippet}
    </Table>
  </SettingsSection>

  <SettingsSection id="export" title="Files" description="Log files rotate daily and are kept for 14 days, up to 100 MB.">
    <SettingsRow label="Log folder" description="Opens the folder in your file manager.">
      {#snippet children()}<Button onclick={openFolder}>Open log folder</Button>{/snippet}
    </SettingsRow>
    <SettingsRow label="Export for a bug report" description="Writes a redacted copy of the log to Downloads, for you to attach yourself.">
      {#snippet children()}<Button onclick={exportLog} loading={exporting}>Export for a bug report</Button>{/snippet}
    </SettingsRow>
    {#if exported}
      <Banner tone="ok">Exported {exported.entries} entries to <code>{exported.path}</code>.</Banner>
    {/if}
  </SettingsSection>
</SettingsLayout>

{#snippet footer()}
  Never logged: prompts, answers, notes, file contents, titles, chat messages, tool arguments, setting values, keys or tokens.
  Error text is redacted and cut short.
{/snippet}

<style>
  code { font-family: var(--font-mono); font-size: var(--font-size-xs); overflow-wrap: anywhere; }
</style>
