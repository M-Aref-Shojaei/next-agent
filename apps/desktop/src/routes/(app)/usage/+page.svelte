<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "$lib/api.ts";
  import {
    Badge, Banner, ProgressBar, SegmentedControl, Select, SettingsLayout, SettingsRow, SettingsSection, Table,
    type Option, type SettingsNavItem, type TableColumn,
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
    type UsageRecent,
    type UsageSummary,
    type UsageTotals,
    type ModelSwitch,
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

  const totalsColumns: TableColumn<UsageTotals>[] = [
    { key: "name", label: "Name", sortable: true, value: (r) => r.key || "—" },
    { key: "requests", label: "Requests", align: "end", sortable: true, value: (r) => r.requests },
    { key: "tokens", label: "Tokens", align: "end", sortable: true, value: (r) => formatTokens(tokens(r)), sortValue: tokens },
    { key: "cost", label: "Cost (est.)", align: "end", sortable: true, value: (r) => formatCost(r.cost) , sortValue: (r) => r.cost },
  ];
  const recentColumns: TableColumn<UsageRecent>[] = [
    { key: "time", label: "Time", sortable: true, nowrap: true, sortValue: (r) => r.ts },
    { key: "role", label: "Role", sortable: true, value: (r) => r.role },
    { key: "model", label: "Provider / model", sortable: true, value: (r) => `${r.provider}/${r.model}` },
    { key: "status", label: "Status", sortable: true, value: (r) => r.status },
    { key: "in", label: "In", align: "end", sortable: true, value: (r) => r.promptTokens },
    { key: "out", label: "Out", align: "end", sortable: true, value: (r) => r.completionTokens },
    { key: "cost", label: "Cost", align: "end", sortable: true, value: (r) => (r.costKnown ? r.cost : null) },
    { key: "latency", label: "Latency", align: "end", sortable: true, nowrap: true, value: (r) => r.latencyMs },
  ];
  const switchColumns: TableColumn<ModelSwitch>[] = [
    { key: "time", label: "Time", nowrap: true, sortable: true, sortValue: (r) => r.ts },
    { key: "combo", label: "Combo", sortable: true, value: (r) => r.combo },
    { key: "task", label: "Task", value: (r) => r.session },
    { key: "route", label: "From → to", value: (r) => `${r.from} → ${r.to}` },
    { key: "why", label: "Why", value: (r) => switchReason(r.reason) },
    { key: "quality", label: "Quality", align: "end", sortable: true, value: (r) => (r.reason === "failover" ? null : r.confidence) },
  ];
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
          <Table label={group.title} columns={totalsColumns} rows={group.rows} rowKey={(r) => r.key} empty="No requests yet." />
        </SettingsSection>
      {/each}
    </div>

    <SettingsSection id="recent" title="Recent requests">
      {#snippet footer()}Costs are estimates from the price table (edit <code>pricing.json</code> in the app data folder to override). Models without a price count as $0.{/snippet}
      <Table label="Recent requests" columns={recentColumns} rows={usage.recent} empty="No requests in this period." sortKey="time" sortDir="desc">
        {#snippet cell(row, col)}
          {#if col.key === "time"}{formatTs(row.ts)}
          {:else if col.key === "model"}{row.provider}/{row.model}{#if row.combo} <span class="muted">({row.combo})</span>{/if}
          {:else if col.key === "status"}<Badge size="sm" tone={row.status >= 200 && row.status < 300 ? "ok" : "danger"}>{row.status || "error"}</Badge>
          {:else if col.key === "in"}{formatTokens(row.promptTokens)}
          {:else if col.key === "out"}{formatTokens(row.completionTokens)}
          {:else if col.key === "cost"}<span title={row.costKnown ? "Estimate from the price table" : "Unknown model: no price"}>{row.costKnown ? formatCost(row.cost) : "—"}</span>
          {:else if col.key === "latency"}{(row.latencyMs / 1000).toFixed(1)} s
          {:else}{col.value?.(row)}{/if}
        {/snippet}
      </Table>
    </SettingsSection>

    <SettingsSection id="switches" title="Model switches (smart combos)">
      <Table label="Model switches" columns={switchColumns} rows={usage.switches} empty="No smart combo changed models in this period.">
        {#snippet cell(row, col)}
          {#if col.key === "time"}{formatTs(row.ts)}
          {:else if col.key === "task"}<code>{row.session || "—"}</code>
          {:else if col.key === "quality"}{row.reason === "failover" ? "—" : row.confidence.toFixed(1)}
          {:else}{col.value?.(row)}{/if}
        {/snippet}
      </Table>
    </SettingsSection>
  {/if}
</SettingsLayout>

<style>
  .muted { color: var(--color-text-tertiary); }
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
</style>
