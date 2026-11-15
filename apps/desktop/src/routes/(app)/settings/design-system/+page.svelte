<script lang="ts">
  // Living style guide: every token and every component state of lib/ds.
  // docs/design/Design System.md is the written half.
  import {
    Badge, Banner, BrandMark, Button, Card, Checkbox, ColorSwatchGroup, Dialog, EmptyState, Field, Icon,
    ICON_NAMES, IconButton, Kbd, List, ListRow, ProgressBar, RadioGroup, SegmentedControl, Select,
    SettingsLayout, SettingsRow, SettingsSection, ShortcutField, Slider, Spinner, StatusDot, Switch, Tabs,
    TextArea, TextField, Tooltip,
    type SettingsNavItem, type Tone,
  } from "$lib/ds/index.ts";

  const nav: SettingsNavItem[] = [
    { id: "colour", label: "Colour", href: "#colour" },
    { id: "type", label: "Type", href: "#type" },
    { id: "space", label: "Space, radii, shadows", href: "#space" },
    { id: "motion", label: "Motion and layers", href: "#motion" },
    { id: "buttons", label: "Buttons", href: "#buttons" },
    { id: "inputs", label: "Inputs", href: "#inputs" },
    { id: "choices", label: "Choices", href: "#choices" },
    { id: "settings", label: "Settings rows", href: "#settings" },
    { id: "feedback", label: "Feedback", href: "#feedback" },
    { id: "content", label: "Content", href: "#content" },
    { id: "overlays", label: "Overlays", href: "#overlays" },
  ];

  const colourGroups: { title: string; tokens: string[] }[] = [
    { title: "Surfaces", tokens: ["bg", "surface", "surface-sunken", "surface-raised", "surface-overlay", "surface-hover", "surface-active", "surface-inverse"] },
    { title: "Text", tokens: ["text", "text-secondary", "text-tertiary", "text-disabled", "text-inverse"] },
    { title: "Borders and focus", tokens: ["border-subtle", "border", "border-strong", "focus", "scrim", "selection", "control-thumb"] },
    { title: "Accent", tokens: ["accent", "accent-hover", "accent-active", "accent-subtle", "accent-text", "on-accent"] },
    ...(["info", "ok", "warn", "danger", "neutral"] as const).map((s) => ({
      title: s[0].toUpperCase() + s.slice(1),
      tokens: [s, `${s}-subtle`, `${s}-border`, `${s}-text`, ...(s === "neutral" ? [] : [`on-${s}`])],
    })),
    { title: "Brand", tokens: ["brand", "brand-2"] },
  ];
  const fontSizes = ["xs", "sm", "md", "lg", "xl", "2xl"];
  const weights = ["regular", "medium", "semibold", "bold"];
  const lineHeights = ["tight", "normal", "relaxed"];
  const spaces = ["0-5", "1", "1-5", "2", "3", "4", "5", "6", "8", "10", "12"];
  const radii = ["xs", "sm", "md", "lg", "xl", "full"];
  const shadows = ["sm", "md", "lg", "xl"];
  const durations = ["instant", "fast", "normal", "slow"];
  const easings = ["standard", "out", "in"];
  const layers = ["base", "raised", "sticky", "dropdown", "overlay", "modal", "popover", "toast", "tooltip"];
  const tones: Tone[] = ["neutral", "accent", "info", "ok", "warn", "danger"];

  // Theme preview: forces data-theme on <html> while this page is open.
  let theme = $state<"system" | "light" | "dark">("system");
  $effect(() => {
    const root = document.documentElement;
    if (theme !== "system") root.dataset.theme = theme;
    return () => delete root.dataset.theme;
  });

  // Component demo state
  let name = $state("Next Agent");
  let apiKey = $state("");
  let notes = $state("");
  let model = $state<"fast" | "smart" | "max">("smart");
  let island = $state(true);
  let sounds = $state(false);
  let agree = $state(true);
  let some = $state(false);
  let someIndeterminate = $state(true);
  let mode = $state<"plan" | "manual" | "auto">("manual");
  let position = $state<"left" | "center" | "right">("center");
  let volume = $state(60);
  let shortcut = $state("Ctrl+Shift+Space");
  let shortcutEmpty = $state("");
  let colour = $state("accent");
  let tab = $state<"general" | "agents" | "about">("general");
  let loading = $state(false);
  let banner = $state(true);
  let confirmOpen = $state(false);
  let dangerOpen = $state(false);
  let progress = $state(42);

  function fakeLoad() {
    loading = true;
    setTimeout(() => (loading = false), 1500);
  }
</script>

<svelte:head><title>Design system · Settings</title></svelte:head>

<SettingsLayout
  title="Design system"
  description="Tokens and components every screen uses, ours and vendored upstreams'. Written guide: docs/design/Design System.md."
  {nav}
>
  {#snippet top()}<a href="/settings">← Settings</a>{/snippet}
  {#snippet actions()}
    <SegmentedControl
      label="Preview theme"
      size="sm"
      bind:value={theme}
      options={[{ value: "system", label: "System" }, { value: "light", label: "Light" }, { value: "dark", label: "Dark" }]}
    />
  {/snippet}

  <!-- ───────────── Tokens ───────────── -->
  <SettingsSection id="colour" title="Colour" description="Semantic colour tokens: --color-<name>. Old names (--bg, --panel, --muted, --accent …) alias these.">
    {#snippet footer()}
      <div class="colour-groups">
        {#each colourGroups as group (group.title)}
          <div class="colour-group">
            <h3>{group.title}</h3>
            <div class="swatches">
              {#each group.tokens as token (token)}
                <div class="swatch">
                  <span class="chip" style:background="var(--color-{token})"></span>
                  <code>--color-{token}</code>
                </div>
              {/each}
            </div>
          </div>
        {/each}
        <div class="colour-group">
          <h3>Brand gradient</h3>
          <div class="swatches">
            <div class="swatch"><span class="chip" style:background="var(--brand-gradient)"></span><code>--brand-gradient</code></div>
            <div class="swatch"><BrandMark size={28} label="Next Agent" /><code>&lt;BrandMark&gt;</code></div>
          </div>
        </div>
      </div>
    {/snippet}
  </SettingsSection>

  <SettingsSection id="type" title="Type" description="System UI font only (strict CSP, no web fonts). Base size 14px.">
    {#snippet footer()}
      <div class="stack">
        {#each fontSizes as s (s)}
          <div class="type-row"><code>--font-size-{s}</code><span style:font-size="var(--font-size-{s})">The agent asks before it acts</span></div>
        {/each}
        <div class="type-row"><code>--font-mono</code><span class="mono">~/.claude/settings.json · git worktree add</span></div>
        {#each weights as w (w)}
          <div class="type-row"><code>--font-weight-{w}</code><span style:font-weight="var(--font-weight-{w})">Weight {w}</span></div>
        {/each}
        {#each lineHeights as l (l)}
          <div class="type-row"><code>--line-height-{l}</code><span class="lh" style:line-height="var(--line-height-{l})">Line height {l}: two lines of text wrap here to show the leading between them clearly.</span></div>
        {/each}
      </div>
    {/snippet}
  </SettingsSection>

  <SettingsSection id="space" title="Space, radii, shadows" description="--space-N is N × 4px. Controls are 24 / 30 / 36px tall (--control-height-sm|md|lg).">
    {#snippet footer()}
      <div class="stack">
        {#each spaces as s (s)}
          <div class="type-row"><code>--space-{s}</code><span class="bar" style:width="var(--space-{s})"></span></div>
        {/each}
        <div class="tiles">
          {#each radii as r (r)}
            <div class="tile" style:border-radius="var(--radius-{r})"><code>--radius-{r}</code></div>
          {/each}
        </div>
        <div class="tiles">
          {#each shadows as s (s)}
            <div class="tile shadow" style:box-shadow="var(--shadow-{s})"><code>--shadow-{s}</code></div>
          {/each}
          <div class="tile shadow" style:box-shadow="var(--focus-ring)"><code>--focus-ring</code></div>
        </div>
        <div class="tiles">
          {#each ["sm", "md", "lg"] as h (h)}
            <div class="ctl" style:height="var(--control-height-{h})"><code>--control-height-{h}</code></div>
          {/each}
        </div>
      </div>
    {/snippet}
  </SettingsSection>

  <SettingsSection id="motion" title="Motion and layers" description="Durations drop to ~0 under prefers-reduced-motion. Hover a row to play it.">
    {#snippet footer()}
      <div class="stack">
        {#each durations as d (d)}
          <div class="type-row motion"><code>--duration-{d}</code><span class="track"><span class="ball" style:transition-duration="var(--duration-{d})"></span></span></div>
        {/each}
        {#each easings as e (e)}
          <div class="type-row motion"><code>--ease-{e}</code><span class="track"><span class="ball" style:transition-timing-function="var(--ease-{e})"></span></span></div>
        {/each}
        <p class="muted small">Layers, low to high: {#each layers as l, i (l)}<code>--z-{l}</code>{i < layers.length - 1 ? " · " : ""}{/each}</p>
      </div>
    {/snippet}
  </SettingsSection>

  <!-- ───────────── Components ───────────── -->
  <SettingsSection id="buttons" title="Buttons" description="One primary per view. Danger only for destructive actions. Ghost for toolbars.">
    {#snippet footer()}
      <div class="stack">
        <div class="demo">
          <Button variant="primary">Primary</Button>
          <Button>Secondary</Button>
          <Button variant="ghost">Ghost</Button>
          <Button variant="danger">Delete</Button>
          <Button variant="primary" disabled>Disabled</Button>
          <Button variant="primary" {loading} onclick={fakeLoad}>{loading ? "Saving" : "Click to load"}</Button>
        </div>
        <div class="demo">
          <Button size="sm">Small</Button>
          <Button>Medium</Button>
          <Button size="lg">Large</Button>
          <Button variant="primary">
            {#snippet icon()}<Icon name="plus" />{/snippet}
            New task
          </Button>
          <Button variant="ghost">
            Open
            {#snippet iconEnd()}<Icon name="external" size={14} />{/snippet}
          </Button>
        </div>
        <div class="demo">
          <IconButton label="Edit"><Icon name="edit" /></IconButton>
          <IconButton label="Copy" variant="secondary"><Icon name="copy" /></IconButton>
          <IconButton label="Delete" variant="danger"><Icon name="trash" /></IconButton>
          <IconButton label="Pin" pressed><Icon name="check" /></IconButton>
          <IconButton label="Settings" size="sm"><Icon name="settings" size={14} /></IconButton>
          <IconButton label="Disabled" disabled><Icon name="more" /></IconButton>
        </div>
        <div class="demo icons">
          {#each ICON_NAMES as n (n)}<span class="icon-cell" title={n}><Icon name={n} /><code>{n}</code></span>{/each}
        </div>
      </div>
    {/snippet}
  </SettingsSection>

  <SettingsSection id="inputs" title="Inputs" description="Always labelled. Hints under the field; an error replaces the hint.">
    {#snippet footer()}
      <div class="grid2">
        <TextField label="Name" bind:value={name} hint="Shown in the island." />
        <TextField label="API key" bind:value={apiKey} type="password" mono placeholder="sk-…" required error={apiKey ? undefined : "Enter a key to connect."} />
        <TextField label="Search" hideLabel placeholder="Search notes">
          {#snippet prefix()}<Icon name="search" size={14} />{/snippet}
        </TextField>
        <TextField label="Budget" type="number" value={20}>
          {#snippet suffix()}USD / month{/snippet}
        </TextField>
        <TextField label="Disabled" value="Read only value" disabled />
        <TextField label="Small" size="sm" placeholder="Compact field" />
        <Select label="Model" bind:value={model} options={[{ value: "fast", label: "Fast" }, { value: "smart", label: "Smart" }, { value: "max", label: "Max" }]} />
        <Select label="Select with error" placeholder="Choose a provider" options={[{ value: "a", label: "Anthropic" }]} error="Pick one." />
        <div class="span2"><TextArea label="System prompt" bind:value={notes} hint="Markdown is allowed." placeholder="You are a careful coding agent…" /></div>
        <Field label="Custom control in a Field" hint="Field wraps anything: it hands the control its id.">
          {#snippet children(ctl)}<Slider id={ctl.id} aria-describedby={ctl.describedby} bind:value={volume} format={(v) => `${v}%`} />{/snippet}
        </Field>
      </div>
    {/snippet}
  </SettingsSection>

  <SettingsSection id="choices" title="Choices">
    {#snippet footer()}
      <div class="grid2">
        <div class="stack">
          <Switch label="Show the island" bind:checked={island} />
          <Switch label="Small switch" size="sm" bind:checked={sounds} />
          <Switch label="Disabled" disabled checked />
        </div>
        <div class="stack">
          <Checkbox label="Accept" bind:checked={agree} />
          <Checkbox label="Select all" description="Indeterminate when some are selected." bind:checked={some} bind:indeterminate={someIndeterminate} />
          <Checkbox label="Disabled" disabled />
        </div>
        <RadioGroup
          label="Permission mode"
          bind:value={mode}
          options={[
            { value: "plan", label: "Plan", description: "Read only; proposes a plan." },
            { value: "manual", label: "Manual", description: "Asks before every write." },
            { value: "auto", label: "Auto", description: "Writes inside a worktree." },
          ]}
        />
        <div class="stack">
          <SegmentedControl label="Island position" bind:value={position} options={[{ value: "left", label: "Left" }, { value: "center", label: "Center" }, { value: "right", label: "Right" }]} />
          <SegmentedControl label="Small" size="sm" value="b" options={[{ value: "a", label: "Day" }, { value: "b", label: "Week" }, { value: "c", label: "Month", disabled: true }]} />
          <Slider aria-label="Volume" bind:value={volume} format={(v) => `${v}%`} />
          <ColorSwatchGroup
            label="Accent colour"
            bind:value={colour}
            swatches={[
              { value: "accent", label: "Accent", color: "var(--color-accent)" },
              { value: "info", label: "Blue", color: "var(--color-info)" },
              { value: "ok", label: "Green", color: "var(--color-ok)" },
              { value: "warn", label: "Orange", color: "var(--color-warn)" },
              { value: "danger", label: "Red", color: "var(--color-danger)" },
              { value: "brand", label: "Brand", color: "var(--color-brand)" },
            ]}
          />
          <div class="demo">
            <ShortcutField label="Toggle island" bind:value={shortcut} />
            <ShortcutField label="Dictation" bind:value={shortcutEmpty} />
          </div>
          <p class="small muted">Shortcuts display as <Kbd keys="Meta+Shift+K" /> or <Kbd keys="Ctrl+Alt+Delete" platform="other" />; single keys: <Kbd keys="Escape" size="sm" />.</p>
        </div>
      </div>
    {/snippet}
  </SettingsSection>

  <SettingsSection id="settings" title="Settings rows" description="The building block of every settings page: label and description left, control right.">
    {#snippet actions()}<Button size="sm" variant="ghost">Reset</Button>{/snippet}
    <SettingsRow label="Show the island" description="The pill around the notch that shows tasks and approvals.">
      {#snippet children(ctl)}<Switch id={ctl.id} aria-describedby={ctl.describedby} bind:checked={island} />{/snippet}
    </SettingsRow>
    <SettingsRow label="Position">
      {#snippet children(ctl)}<SegmentedControl labelledby={ctl.labelId} bind:value={position} options={[{ value: "left", label: "Left" }, { value: "center", label: "Center" }, { value: "right", label: "Right" }]} />{/snippet}
    </SettingsRow>
    <SettingsRow label="Default model" description="Used when a task doesn't pick one.">
      {#snippet children(ctl)}<Select id={ctl.id} aria-describedby={ctl.describedby} bind:value={model} options={[{ value: "fast", label: "Fast" }, { value: "smart", label: "Smart" }, { value: "max", label: "Max" }]} />{/snippet}
    </SettingsRow>
    <SettingsRow label="Volume">
      {#snippet children(ctl)}<Slider id={ctl.id} bind:value={volume} format={(v) => `${v}%`} />{/snippet}
    </SettingsRow>
    <SettingsRow label="Toggle island" description="Global shortcut.">
      {#snippet children(ctl)}<ShortcutField id={ctl.id} describedby={ctl.describedby} bind:value={shortcut} />{/snippet}
    </SettingsRow>
    <SettingsRow label="Router URL" error={name ? undefined : "Required"} description="OpenAI-compatible base URL.">
      {#snippet children(ctl)}<TextField id={ctl.id} aria-describedby={ctl.describedby} mono placeholder="http://localhost:20128/v1" />{/snippet}
    </SettingsRow>
    <SettingsRow label="Accent" description="Island highlight colour.">
      {#snippet children(ctl)}
        <ColorSwatchGroup labelledby={ctl.labelId} bind:value={colour} swatches={[
          { value: "accent", label: "Accent", color: "var(--color-accent)" },
          { value: "ok", label: "Green", color: "var(--color-ok)" },
          { value: "brand", label: "Brand", color: "var(--color-brand)" },
        ]} />
      {/snippet}
    </SettingsRow>
    <SettingsRow label="Claude Code hooks" description="Status row: state on the left, action on the right.">
      {#snippet children()}<StatusDot tone="ok" text="Installed" /><Button size="sm">Reinstall</Button>{/snippet}
    </SettingsRow>
    <SettingsRow label="Instructions" description="Stacked row for wide controls." stacked>
      {#snippet children(ctl)}<TextArea id={ctl.id} aria-describedby={ctl.describedby} rows={3} placeholder="Always answer in English." />{/snippet}
    </SettingsRow>
    <SettingsRow label="Disabled row" description="Not available on this platform." disabled>
      {#snippet children(ctl)}<Switch id={ctl.id} disabled />{/snippet}
    </SettingsRow>
  </SettingsSection>

  <SettingsSection id="feedback" title="Feedback">
    {#snippet footer()}
      <div class="stack">
        {#if banner}
          <Banner tone="info" title="Update available" ondismiss={() => (banner = false)}>
            Coucou published 3 commits since the pinned one.
            {#snippet actions()}<Button size="sm">Review</Button>{/snippet}
          </Banner>
        {:else}
          <Button size="sm" onclick={() => (banner = true)}>Show the dismissed banner</Button>
        {/if}
        <Banner tone="ok">Vault path saved.</Banner>
        <Banner tone="warn" title="Hooks not installed">Claude Code sessions won't reach the island.</Banner>
        <Banner tone="danger" title="Audit chain broken">Entry #41 doesn't match its hash.</Banner>
        <div class="demo">
          {#each tones as t (t)}<Badge tone={t}>{t}</Badge>{/each}
        </div>
        <div class="demo">
          {#each tones as t (t)}<Badge tone={t} variant="solid">{t}</Badge>{/each}
        </div>
        <div class="demo">
          {#each tones as t (t)}<Badge tone={t} variant="outline" size="sm">{t}</Badge>{/each}
        </div>
        <div class="demo">
          {#each tones as t (t)}<StatusDot tone={t} text={t} />{/each}
          <StatusDot tone="info" pulse text="working" />
          <StatusDot tone="ok" label="Connected" />
        </div>
        <div class="demo">
          <Spinner size="sm" /> <Spinner /> <Spinner size="lg" />
        </div>
        <div class="grid2">
          <ProgressBar label="Indexing vault" value={progress} showValue />
          <ProgressBar label="Downloading model" tone="ok" value={88} showValue />
          <ProgressBar label="Budget used" tone="warn" value={92} showValue />
          <ProgressBar label="Waiting" />
        </div>
        <div class="demo"><Button size="sm" onclick={() => (progress = (progress + 17) % 101)}>Advance progress</Button></div>
      </div>
    {/snippet}
  </SettingsSection>

  <SettingsSection id="content" title="Content">
    {#snippet footer()}
      <div class="stack">
        <Card variant="hero" title="Next Agent" description="A local-first coding agent. Version 0.1.0.">
          {#snippet media()}<BrandMark size={44} />{/snippet}
          {#snippet actions()}<Button size="sm">Check for updates</Button>{/snippet}
        </Card>
        <div class="grid2">
          <Card title="Card" description="Default card with header actions.">
            {#snippet actions()}<IconButton label="More" size="sm"><Icon name="more" /></IconButton>{/snippet}
            <p class="small">Cards group related content on a page.</p>
          </Card>
          <Card variant="subtle" title="Subtle card"><p class="small muted">Sunken; for secondary content.</p></Card>
        </div>
        <Tabs label="Overlay settings" bind:value={tab} tabs={[{ id: "general", label: "General" }, { id: "agents", label: "Agents", badge: 3 }, { id: "about", label: "About" }]}>
          {#snippet children(active)}<p class="small muted">Panel for <strong>{active}</strong>. Arrow keys move between tabs.</p>{/snippet}
        </Tabs>
        <List label="Telegram bots">
          <ListRow title="@next_agent_bot" description="Paired with 2 chats">
            {#snippet leading()}<StatusDot tone="ok" label="Online" />{/snippet}
            {#snippet actions()}
              <IconButton label="Edit bot" size="sm"><Icon name="edit" size={14} /></IconButton>
              <IconButton label="Remove bot" size="sm" variant="danger"><Icon name="trash" size={14} /></IconButton>
            {/snippet}
          </ListRow>
          <ListRow title="Selected row" description="onclick makes the row a button" selected onclick={() => {}} meta="2m ago" />
          <ListRow title="Disabled row" disabled description="Unavailable">
            {#snippet children()}<Badge size="sm" tone="warn">paused</Badge>{/snippet}
          </ListRow>
        </List>
        <Card padding="none">
          <EmptyState title="No permission rules" description="&quot;Always for this workspace&quot; approvals appear here.">
            {#snippet icon()}<Icon name="info" size={20} />{/snippet}
            {#snippet actions()}<Button variant="primary" size="sm">Add a rule</Button>{/snippet}
          </EmptyState>
        </Card>
      </div>
    {/snippet}
  </SettingsSection>

  <SettingsSection id="overlays" title="Overlays">
    {#snippet footer()}
      <div class="demo">
        <Button onclick={() => (confirmOpen = true)}>Open confirm dialog</Button>
        <Button variant="danger" onclick={() => (dangerOpen = true)}>Open danger dialog</Button>
        <Tooltip text="Tooltips explain icons; never hide required information in them.">
          {#snippet children(t)}<IconButton label="Help" aria-describedby={t.describedby}><Icon name="info" /></IconButton>{/snippet}
        </Tooltip>
        <Tooltip text="Below the trigger" placement="bottom">
          {#snippet children(t)}<Button aria-describedby={t.describedby}>Tooltip below</Button>{/snippet}
        </Tooltip>
      </div>
    {/snippet}
  </SettingsSection>
</SettingsLayout>

<Dialog
  bind:open={confirmOpen}
  title="Install Claude Code hooks?"
  description="This adds Next Agent's hook to ~/.claude/settings.json and saves a dated backup first."
  confirmLabel="Install"
  onconfirm={() => new Promise((r) => setTimeout(r, 800))}
/>
<Dialog
  bind:open={dangerOpen}
  tone="danger"
  title="Delete this board?"
  description="The board and its 12 stickies are removed. This can't be undone."
  confirmLabel="Delete board"
/>

<style>
  .stack { display: grid; gap: var(--space-3); }
  .demo { display: flex; flex-wrap: wrap; align-items: center; gap: var(--space-2); }
  .grid2 { display: grid; grid-template-columns: repeat(auto-fit, minmax(240px, 1fr)); gap: var(--space-4); align-items: start; }
  .span2 { grid-column: 1 / -1; }
  .small { margin: 0; font-size: var(--font-size-sm); }
  code { font-size: var(--font-size-xs); color: var(--color-text-secondary); }

  .colour-groups { display: grid; gap: var(--space-4); }
  .colour-group h3 { font-size: var(--font-size-sm); margin-bottom: var(--space-2); color: var(--color-text); }
  .swatches { display: grid; grid-template-columns: repeat(auto-fill, minmax(170px, 1fr)); gap: var(--space-2); }
  .swatch { display: flex; align-items: center; gap: var(--space-2); min-width: 0; }
  .swatch code { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .chip { flex: none; width: 28px; height: 28px; border-radius: var(--radius-md); border: var(--border-width) solid var(--color-border); }

  .type-row { display: grid; grid-template-columns: 170px minmax(0, 1fr); align-items: center; gap: var(--space-3); color: var(--color-text); }
  .lh { max-width: 34ch; }
  .bar { display: block; height: 12px; border-radius: var(--radius-xs); background: var(--color-accent); }
  .tiles { display: flex; flex-wrap: wrap; gap: var(--space-3); }
  .tile {
    display: grid;
    place-items: center;
    width: 112px;
    height: 64px;
    border: var(--border-width) solid var(--color-border);
    background: var(--color-surface);
  }
  .tile.shadow { border-color: transparent; border-radius: var(--radius-lg); }
  .ctl { display: grid; place-items: center; padding: 0 var(--space-3); border: var(--border-width) dashed var(--color-border-strong); border-radius: var(--radius-md); }

  .track { position: relative; display: block; height: 14px; border-radius: var(--radius-full); background: var(--color-surface-sunken); }
  .ball {
    position: absolute;
    left: 0;
    top: 1px;
    width: 12px;
    height: 12px;
    border-radius: var(--radius-full);
    background: var(--color-accent);
    transition-property: left;
    transition-duration: var(--duration-slow);
    transition-timing-function: var(--ease-standard);
  }
  .motion:hover .ball { left: calc(100% - 12px); }

  .icons { gap: var(--space-1); }
  .icon-cell {
    display: grid;
    justify-items: center;
    gap: var(--space-1);
    width: 84px;
    padding: var(--space-2) 0;
    border-radius: var(--radius-md);
    color: var(--color-text);
  }
  .icon-cell:hover { background: var(--color-surface-hover); }

  @media (max-width: 560px) {
    .type-row { grid-template-columns: minmax(0, 1fr); gap: var(--space-1); }
  }
</style>
