<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "$lib/api.ts";
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
    { days: 1, label: "Today" },
    { days: 7, label: "7 days" },
    { days: 30, label: "30 days" },
    { days: 0, label: "All time" },
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

  // The chart always covers a whole period; "today" and "all" show 7 / 30 days.
  const chartDays = $derived(days === 7 ? 7 : 30);
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

<div class="page">
  <h1>Usage</h1>
  <p><a href="/settings/providers">← Providers</a></p>
  <div class="periods" role="group" aria-label="Period">
    {#each PERIODS as period (period.days)}
      <button type="button" aria-pressed={days === period.days} onclick={() => pick(period.days)}>{period.label}</button>
    {/each}
  </div>
  {#if message}<p role="alert">{message}</p>{/if}

  {#if usage}
    <section class="cards" aria-label="Totals">
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
    </section>

    {#if usage.budgets.length > 0}
      <section aria-labelledby="h-budgets">
        <h2 id="h-budgets">Budgets</h2>
        {#each usage.budgets as status, i (i)}
          <div class="budget">
            <span>{budgetLabel(status.budget)}</span>
            <meter min="0" max="100" low="80" high="99" optimum="0" value={budgetPercent(status)} aria-label={budgetLabel(status.budget)}></meter>
            <span class={status.level === "ok" ? "" : "bad"}>{formatCost(status.spent)} of {formatCost(status.budget.limitUsd)}
              {#if status.level === "warn"} · over 80%{:else if status.level === "over"} · reached{/if}</span>
          </div>
        {/each}
        <p class="muted"><a href="/settings/providers">Change budgets</a></p>
      </section>
    {/if}

    <section aria-labelledby="h-chart">
      <div class="chart-head">
        <h2 id="h-chart">Last {chartDays} days</h2>
        <label for="metric" class="sr-only">Chart metric</label>
        <select id="metric" bind:value={metric}>
          <option value="tokens">Tokens</option>
          <option value="cost">Cost</option>
          <option value="requests">Requests</option>
        </select>
      </div>
      <svg class="chart" viewBox="0 0 {bars.length * 10} 60" preserveAspectRatio="none" role="img"
        aria-label="Daily {metric} for the last {chartDays} days">
        {#each bars as bar, i (bar.day)}
          <rect x={i * 10 + 1} y={58 - bar.height * 56} width="8" height={Math.max(bar.height * 56, bar.value > 0 ? 1 : 0.3)}>
            <title>{bar.day}: {barValue(bar.value)}</title>
          </rect>
        {/each}
      </svg>
      <div class="axis muted"><span>{bars[0]?.day ?? ""}</span><span>{bars[bars.length - 1]?.day ?? ""}</span></div>
    </section>

    <div class="breakdowns">
      {#each breakdowns as group (group.title)}
        <section aria-label={group.title}>
          <h2>{group.title}</h2>
          {#if group.rows.length === 0}
            <p class="muted">No requests yet.</p>
          {:else}
            <table>
              <thead><tr><th>Name</th><th>Requests</th><th>Tokens</th><th>Cost (est.)</th></tr></thead>
              <tbody>
                {#each group.rows as row (row.key)}
                  <tr><td>{row.key || "—"}</td><td>{row.requests}</td><td>{formatTokens(tokens(row))}</td><td>{formatCost(row.cost)}</td></tr>
                {/each}
              </tbody>
            </table>
          {/if}
        </section>
      {/each}
    </div>

    <section aria-labelledby="h-recent">
      <h2 id="h-recent">Recent requests</h2>
      {#if usage.recent.length === 0}
        <p class="muted">No requests in this period.</p>
      {:else}
        <table>
          <thead><tr><th>Time</th><th>Role</th><th>Provider / model</th><th>Status</th><th>In</th><th>Out</th><th>Cost</th><th>Latency</th></tr></thead>
          <tbody>
            {#each usage.recent as row, i (i)}
              <tr>
                <td>{formatTs(row.ts)}</td>
                <td>{row.role}</td>
                <td>{row.provider}/{row.model}{#if row.combo} <span class="muted">({row.combo})</span>{/if}</td>
                <td class={row.status >= 200 && row.status < 300 ? "ok" : "bad"}>{row.status || "error"}</td>
                <td>{formatTokens(row.promptTokens)}</td>
                <td>{formatTokens(row.completionTokens)}</td>
                <td title={row.costKnown ? "Estimate from the price table" : "Unknown model: no price"}>{row.costKnown ? formatCost(row.cost) : "—"}</td>
                <td>{(row.latencyMs / 1000).toFixed(1)} s</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}
      <p class="muted">Costs are estimates from the price table (edit <code>pricing.json</code> in the app data folder to override). Models without a price count as $0.</p>
    </section>

    <section aria-labelledby="h-switches">
      <h2 id="h-switches">Model switches (smart combos)</h2>
      {#if usage.switches.length === 0}
        <p class="muted">No smart combo changed models in this period.</p>
      {:else}
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
      {/if}
    </section>
  {/if}
</div>

<style>
  .page { padding: 20px 28px; overflow: auto; max-width: 1000px; display: grid; gap: 14px; align-content: start; }
  section { border: 1px solid var(--border); border-radius: 8px; padding: 12px 14px; background: var(--panel); }
  .periods { display: flex; gap: 6px; }
  .periods button[aria-pressed="true"] { background: var(--accent); border-color: var(--accent); color: #fff; }
  .cards { display: grid; grid-template-columns: repeat(auto-fit, minmax(150px, 1fr)); gap: 12px; }
  .cards div { display: grid; gap: 2px; }
  .cards strong { font-size: 22px; }
  .chart-head { display: flex; justify-content: space-between; align-items: center; }
  select { font: inherit; background: var(--panel); color: var(--text); border: 1px solid var(--border); border-radius: 6px; padding: 4px 6px; }
  .chart { width: 100%; height: 140px; margin-top: 8px; }
  .chart rect { fill: var(--accent); }
  .axis { display: flex; justify-content: space-between; font-size: 12px; }
  .breakdowns { display: grid; grid-template-columns: repeat(auto-fit, minmax(280px, 1fr)); gap: 14px; }
  table { width: 100%; border-collapse: collapse; margin-top: 8px; font-size: 13px; }
  th, td { text-align: left; padding: 4px 6px; border-bottom: 1px solid var(--border); }
  .ok { color: var(--ok); } .bad { color: var(--danger); }
  .budget { display: grid; grid-template-columns: minmax(160px, auto) 1fr auto; gap: 10px; align-items: center; margin-top: 6px; }
  meter { width: 100%; }
</style>
