<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "$lib/api.ts";
  import {
    Badge, Banner, ProgressBar, SegmentedControl, Select, SettingsLayout, SettingsRow, SettingsSection,
    type Option, type SettingsNavItem,
  } from "$lib/ds/index.ts";
  import {
    dailyBars,
    formatCost,
    formatTs,
    formatTokens,
    savedPercent,
    budgetLabel,
    budgetPercent,
    cacheHitRate,
    switchReason,
    type UsageSummary,
    type UsageTotals,
  } from "$lib/gateway.ts";

  type Metric = "cost" | "tokens" | "requests";
  const PERIODS = [
    { days: 1, chartDays: 30, label: "Today" },
    { days: 7, chartDays: 7, label: "7 days" },
    { days: 30, chartDays: 30, label: "30 days" },
    { days: 0, chartDays: 30, label: "All time" },
  ];

  const periodOptions: Option[] = PERIODS.map((p) => ({ value: String(p.days), label: p.label }));
  const metricOptions: Option<Metric>[] = [
    { value: "tokens", label: "Tokens" },
    { value: "cost", label: "Cost" },
    { value: "requests", label: "Requests" },
  ];
  const levelTone = { ok: "ok", warn: "warn", over: "danger" } as const;
  const nav: SettingsNavItem[] = [
    { id: "totals", label: "Totals", href: "#totals" },
    { id: "budgets", label: "Budgets", href: "#budgets" },
    { id: "chart", label: "Chart", href: "#chart" },
    { id: "breakdowns", label: "Breakdowns", href: "#breakdowns" },
    { id: "recent", label: "Recent requests", href: "#recent" },
    { id: "switches", label: "Model switches", href: "#switches" },
  ];

  let days = $state(7);
  let metric = $state<Metric>("tokens");
  let usage = $state<UsageSummary | null>(null);
  let message = $state("");

  async function load() {
    try {
      usage = await api.usageSummary(days);
      message = "";
    } catch (e) {
      message = String(e);
    }
  }
  onMount(() => void load());

  function pick(next: number) {
    days = next;
    void load();
  }

  // The chart always covers a whole period; "today" and "all" show 30 days.
  const chartDays = $derived(PERIODS.find((p) => p.days === days)?.chartDays ?? 30);
  const bars = $derived(usage ? dailyBars(usage.daily, chartDays, metric) : []);
  const barValue = (value: number) =>
    metric === "cost" ? formatCost(value) : metric === "tokens" ? formatTokens(value) : String(value);
  const breakdowns = $derived(
    usage
      ? [
          { title: "By role", rows: usage.byRole },
          { title: "By provider", rows: usage.byProvider },
          { title: "By model", rows: usage.byModel },
        ]
      : [],
  );
  const tokens = (t: UsageTotals) => t.promptTokens + t.completionTokens;
</script>

<SettingsLayout title="Usage" {nav} description="What the gateway sent, and what it cost (estimates from the price table).">
  {#snippet top()}<a href="/settings/providers">← Providers</a>{/snippet}
  {#snippet actions()}
    <SegmentedControl label="Period" size="sm" options={periodOptions} bind:value={() => String(days), (v) => pick(Number(v))} />
  {/snippet}

  {#if message}<Banner tone="danger">{message}</Banner>{/if}

  {#if usage}
    <SettingsSection id="totals" title="Totals">
      <div class="cards">
        <div><span class="muted">Requests</span><strong>{usage.totals.requests}</strong>
          {#if usage.totals.failures}<span class="muted">{usage.totals.failures} failed</span>{/if}</div>
        <div><span class="muted">Input tokens</span><strong>{formatTokens(usage.totals.promptTokens)}</strong>
          <span class="muted">{formatTokens(usage.totals.cachedTokens)} cached</span></div>
        <div><span class="muted">Output tokens</span><strong>{formatTokens(usage.totals.completionTokens)}</strong></div>
        <div><span class="muted">Cost (estimate)</span><strong>{formatCost(usage.totals.cost)}</strong></div>
        <div><span class="muted">Prompt cache hit rate</span><strong>{cacheHitRate(usage.totals)}%</strong>
          <span class="muted">of input tokens</span></div>
        <div><span class="muted">Response cache</span><strong>{usage.cacheHits} hits</strong>
          <span class="muted">saved {formatTokens(usage.cacheSavedTokens)} tokens · {formatCost(usage.cacheSavedCost)}</span></div>
        <div><span class="muted">Tool output saved (RTK)</span>
          <strong>{savedPercent(usage.saverBytesBefore, usage.saverBytesAfter)}%</strong>
          <span class="muted">{formatTokens(usage.saverBytesBefore - usage.saverBytesAfter)} bytes</span></div>
      </div>
    </SettingsSection>

    {#if usage.budgets.length > 0}
      <SettingsSection id="budgets" title="Budgets">
        {#snippet footer()}<a href="/settings/providers#budgets">Change budgets</a>{/snippet}
        {#each usage.budgets as status, i (i)}
          <SettingsRow
            label={budgetLabel(status.budget)}
            description="{formatCost(status.spent)} of {formatCost(status.budget.limitUsd)}{status.level === 'warn' ? ' · over 80%' : status.level === 'over' ? ' · reached' : ''}"
          >
            {#snippet children()}
              <div class="meter">
                <ProgressBar label={budgetLabel(status.budget)} hideLabel value={budgetPercent(status)} tone={levelTone[status.level]} />
              </div>
            {/snippet}
          </SettingsRow>
        {/each}
      </SettingsSection>
    {/if}

    <SettingsSection id="chart" title="Last {chartDays} days">
      {#snippet actions()}
        <Select aria-label="Chart metric" size="sm" options={metricOptions} bind:value={metric} />
      {/snippet}
      <div class="chart-box">
        <svg class="chart" viewBox="0 0 {bars.length * 10} 60" preserveAspectRatio="none" role="img"
          aria-label="Daily {metric} for the last {chartDays} days">
          {#each bars as bar, i (bar.day)}
            <rect x={i * 10 + 1} y={58 - bar.height * 56} width="8" height={Math.max(bar.height * 56, bar.value > 0 ? 1 : 0.3)}>
              <title>{bar.day}: {barValue(bar.value)}</title>
            </rect>
          {/each}
        </svg>
        <div class="axis muted"><span>{bars[0]?.day ?? ""}</span><span>{bars[bars.length - 1]?.day ?? ""}</span></div>
      </div>
    </SettingsSection>

    <div id="breakdowns" class="breakdowns">
      {#each breakdowns as group (group.title)}
        <SettingsSection title={group.title}>
          {#if group.rows.length === 0}
            <p class="note">No requests yet.</p>
          {:else}
            <div class="scroll">
              <table>
                <thead><tr><th>Name</th><th>Requests</th><th>Tokens</th><th>Cost (est.)</th></tr></thead>
                <tbody>
                  {#each group.rows as row (row.key)}
                    <tr><td>{row.key || "—"}</td><td>{row.requests}</td><td>{formatTokens(tokens(row))}</td><td>{formatCost(row.cost)}</td></tr>
                  {/each}
                </tbody>
              </table>
            </div>
          {/if}
        </SettingsSection>
      {/each}
    </div>

    <SettingsSection id="recent" title="Recent requests">
      {#snippet footer()}Costs are estimates from the price table (edit <code>pricing.json</code> in the app data folder to override). Models without a price count as $0.{/snippet}
      {#if usage.recent.length === 0}
        <p class="note">No requests in this period.</p>
      {:else}
        <div class="scroll">
          <table>
            <thead><tr><th>Time</th><th>Role</th><th>Provider / model</th><th>Status</th><th>In</th><th>Out</th><th>Cost</th><th>Latency</th></tr></thead>
            <tbody>
              {#each usage.recent as row, i (i)}
                <tr>
                  <td>{formatTs(row.ts)}</td>
                  <td>{row.role}</td>
                  <td>{row.provider}/{row.model}{#if row.combo} <span class="muted">({row.combo})</span>{/if}</td>
                  <td><Badge size="sm" tone={row.status >= 200 && row.status < 300 ? "ok" : "danger"}>{row.status || "error"}</Badge></td>
                  <td>{formatTokens(row.promptTokens)}</td>
                  <td>{formatTokens(row.completionTokens)}</td>
                  <td title={row.costKnown ? "Estimate from the price table" : "Unknown model: no price"}>{row.costKnown ? formatCost(row.cost) : "—"}</td>
                  <td>{(row.latencyMs / 1000).toFixed(1)} s</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    </SettingsSection>

    <SettingsSection id="switches" title="Model switches (smart combos)">
      {#if usage.switches.length === 0}
        <p class="note">No smart combo changed models in this period.</p>
      {:else}
        <div class="scroll">
          <table>
            <thead><tr><th>Time</th><th>Combo</th><th>Task</th><th>From → to</th><th>Why</th><th>Quality</th></tr></thead>
            <tbody>
              {#each usage.switches as row, i (i)}
                <tr>
                  <td>{formatTs(row.ts)}</td>
                  <td>{row.combo}</td>
                  <td><code>{row.session || "—"}</code></td>
                  <td>{row.from} → {row.to}</td>
                  <td>{switchReason(row.reason)}</td>
                  <td>{row.reason === "failover" ? "—" : row.confidence.toFixed(1)}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    </SettingsSection>
  {/if}
</SettingsLayout>

<style>
  .muted { color: var(--color-text-tertiary); }
  .note { margin: 0; padding: var(--space-3) var(--space-4); color: var(--color-text-secondary); font-size: var(--font-size-sm); }
  code { font-family: var(--font-mono); font-size: var(--font-size-xs); }
  .cards { display: grid; grid-template-columns: repeat(auto-fit, minmax(150px, 1fr)); gap: var(--space-4); padding: var(--space-4); }
  .cards div { display: grid; gap: var(--space-0-5); align-content: start; font-size: var(--font-size-sm); }
  .cards strong { font-size: var(--font-size-xl); }
  .meter { width: 200px; max-width: 100%; }
  .chart-box { padding: var(--space-4); }
  .chart { width: 100%; height: 140px; }
  .chart rect { fill: var(--color-accent); }
  .axis { display: flex; justify-content: space-between; font-size: var(--font-size-xs); margin-top: var(--space-1); }
  .breakdowns { display: grid; gap: var(--space-8); }
  .scroll { overflow-x: auto; }
  table { width: 100%; border-collapse: collapse; font-size: var(--font-size-sm); }
  th, td { text-align: left; padding: var(--space-2) var(--space-3); border-bottom: var(--border-width) solid var(--color-border-subtle); }
  th { color: var(--color-text-secondary); font-weight: var(--font-weight-medium); white-space: nowrap; }
  tbody tr:last-child td { border-bottom: 0; }
</style>
