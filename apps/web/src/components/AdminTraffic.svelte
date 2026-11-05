<script lang="ts">
  // Admin → Traffic: cuanto se le pega al server, desde que IP y que usuario. Lee
  // /api/admin/metrics (agregados por hora). Tocar una IP, un usuario o una ruta abre los logs
  // filtrados por eso.
  import { Activity, ArrowDownUp, Gauge, RefreshCw, ShieldAlert, TriangleAlert } from "@lucide/svelte";
  import { onDestroy } from "svelte";
  import { api, errText } from "../lib/api";
  import { bytes } from "../lib/format";
  import { t as tr } from "../lib/i18n.svelte";
  import { rise } from "../lib/motion";
  import { read, write } from "../lib/store";
  import { agoMs, buckets, count, dur, fullDate, pct } from "../lib/traffic";
  import type { LogFilters, Metrics, MetricsRange } from "../lib/types";
  import Seg from "./Seg.svelte";
  import TrafficChart, { type ChartMetric } from "./TrafficChart.svelte";

  let { onfilter }: { onfilter: (f: LogFilters) => void } = $props();

  // Las etiquetas son getters: la lista es fija y el texto sigue al idioma de la interfaz.
  const RANGES: { value: MetricsRange; label: string }[] = [
    { value: "24h", label: "24 h" },
    { value: "7d", get label() { return tr("admin.tr.7d"); } },
    { value: "30d", get label() { return tr("admin.tr.30d"); } },
    { value: "90d", get label() { return tr("admin.tr.90d"); } },
  ];
  const METRICS: { value: ChartMetric; label: string }[] = [
    { value: "requests", get label() { return tr("admin.tr.requests"); } },
    { value: "bytes", get label() { return tr("admin.tr.data"); } },
    { value: "latency", get label() { return tr("admin.tr.latency"); } },
  ];

  const RANGE_KEY = "mythos.admin.range";
  const saved = read(RANGE_KEY) as MetricsRange | null;
  let range = $state<MetricsRange>(saved && RANGES.some((r) => r.value === saved) ? saved : "24h");
  let metric = $state<ChartMetric>("requests");
  let allRoutes = $state(false);
  let m = $state<Metrics | null>(null);
  let error = $state("");
  let loading = $state(false);
  let at = $state(0);
  let now = $state(Date.now());

  let ctrl: AbortController | null = null;
  async function load() {
    ctrl?.abort();
    const c = (ctrl = new AbortController());
    loading = true;
    try {
      const r = await api.metrics(range, c.signal);
      if (c.signal.aborted) return;
      m = r;
      error = "";
      at = Date.now();
    } catch (e) {
      if (!c.signal.aborted) error = errText(e);
    } finally {
      if (ctrl === c) loading = false;
    }
  }
  function setRange(r: MetricsRange) {
    range = r;
    write(RANGE_KEY, r);
    load();
  }
  load();

  // Se actualiza solo cada minuto con la pestaña visible (los agregados son por hora).
  const timer = setInterval(() => {
    now = Date.now();
    if (!document.hidden && Date.now() - at > 60_000) load();
  }, 15_000);
  onDestroy(() => {
    clearInterval(timer);
    ctrl?.abort();
  });

  const series = $derived(m ? buckets(m, range) : null);
  const t = $derived(m?.totals);
  const errRate = $derived(t && t.requests ? t.errors / t.requests : 0);
  const maxIp = $derived(Math.max(1, ...(m?.top_ips.map((x) => x.requests) ?? [1])));
  const maxUser = $derived(Math.max(1, ...(m?.top_users.map((x) => x.requests) ?? [1])));
  const failedByIp = $derived(new Map((m?.failed_logins ?? []).map((f) => [f.ip, f])));
  const failedTotal = $derived((m?.failed_logins ?? []).reduce((a, f) => a + f.count, 0));

  // Estados agrupados por clase (2xx, 3xx, 4xx, 5xx) para la barra de abajo.
  const classes = $derived.by(() => {
    const c = [
      { k: "2xx", n: 0, tone: "ok" },
      { k: "3xx", n: 0, tone: "muted" },
      { k: "4xx", n: 0, tone: "warn" },
      { k: "5xx", n: 0, tone: "bad" },
    ];
    for (const s of m?.status ?? []) {
      const i = Math.floor(s.status / 100) - 2;
      if (c[i]) c[i].n += s.requests;
    }
    return c;
  });
  const statusTotal = $derived(classes.reduce((a, c) => a + c.n, 0));
  const topStatuses = $derived([...(m?.status ?? [])].filter((s) => s.status >= 400).sort((a, b) => b.requests - a.requests).slice(0, 6));

  const methodTone = (meth: string) => (meth === "GET" ? "" : meth === "DELETE" ? "bad" : "accent");
</script>

<div class="bar">
  <Seg value={range} options={RANGES} onchange={setRange} />
  <div class="grow"></div>
  <span class="faint small upd" title={at ? fullDate(at) : ""}>{at ? tr("admin.tr.updated", { ago: agoMs(at, now) }) : ""}</span>
  <button class="icon-btn" title={tr("admin.tr.refresh")} onclick={load} disabled={loading}><RefreshCw size={15} class={loading ? "spin" : ""} /></button>
</div>

{#if error && !m}
  <div class="empty card"><h3>{tr("admin.tr.loadError")}</h3><span>{error}</span><button class="btn" onclick={load}>{tr("common.retry")}</button></div>
{:else}
  <!-- Numeros grandes -->
  <div class="kpis">
    <div class="card kpi">
      <div class="k-head"><Activity size={14} />{tr("admin.tr.requests")}</div>
      {#if t}
        <div class="k-n">{count(t.requests)}</div>
        <div class="k-sub">{m!.top_ips.length >= 20 ? "20+" : m!.top_ips.length} IPs · {tr("admin.tr.nUsers", { n: m!.top_users.length })}</div>
      {:else}<div class="skel line k-skel"></div>{/if}
    </div>
    <div class="card kpi" class:alert={errRate >= 0.01}>
      <div class="k-head"><TriangleAlert size={14} />{tr("admin.tr.errors")}</div>
      {#if t}
        <div class="k-n" class:bad={errRate >= 0.01}>{pct(t.errors, t.requests)}</div>
        <div class="k-sub">{tr("admin.tr.errSplit", { s: count(t.errors), c: count(t.client_errors) })}</div>
      {:else}<div class="skel line k-skel"></div>{/if}
    </div>
    <div class="card kpi">
      <div class="k-head"><ArrowDownUp size={14} />{tr("admin.tr.dataSent")}</div>
      {#if t}
        <div class="k-n">{bytes(t.bytes_out) || "0 B"}</div>
        <div class="k-sub">{t.requests ? tr("admin.tr.perRequest", { size: bytes(t.bytes_out / t.requests) || "0 B" }) : tr("admin.tr.noRequestsShort")}</div>
      {:else}<div class="skel line k-skel"></div>{/if}
    </div>
    <div class="card kpi">
      <div class="k-head"><Gauge size={14} />{tr("admin.tr.p95")}</div>
      {#if t}
        <div class="k-n">{dur(t.p95_ms)}</div>
        <div class="k-sub">{tr("admin.tr.median", { d: dur(t.p50_ms) })}</div>
      {:else}<div class="skel line k-skel"></div>{/if}
    </div>
  </div>

  <!-- Grafico por hora -->
  <div class="card chart-card">
    <div class="c-head">
      <div class="c-title">
        {metric === "requests" ? tr("admin.tr.requests") : metric === "bytes" ? tr("admin.tr.dataSent") : tr("admin.tr.p95")}
        {#if series}<span class="faint">{series.hours >= 24 ? tr("admin.tr.perDay") : series.hours === 1 ? tr("admin.tr.perHour") : tr("admin.tr.perHours", { n: series.hours })}</span>{/if}
      </div>
      {#if metric === "requests"}
        <div class="legend">
          <span><i class="sw main"></i>{tr("admin.tr.requests")}</span>
          <span><i class="sw err"></i>{tr("admin.tr.serverErrors")}</span>
        </div>
      {/if}
      <div class="grow"></div>
      <Seg bind:value={metric} options={METRICS} />
    </div>
    {#if series}
      {#key range}
        <div in:rise={{ y: 4 }}><TrafficChart data={series.list} hours={series.hours} {metric} /></div>
      {/key}
    {:else}
      <div class="skel chart-skel"></div>
    {/if}
  </div>

  {#if m && m.failed_logins.length}
    <div class="card fails" in:rise>
      <div class="f-head">
        <ShieldAlert size={16} />
        <b>{tr("admin.tr.failedLogins", { n: failedTotal })}</b>
        <span class="muted">{tr("admin.tr.failedFrom", { n: m.failed_logins.length })}</span>
      </div>
      <div class="f-list">
        {#each m.failed_logins as f (f.ip)}
          <button class="f-row" onclick={() => onfilter({ ip: f.ip })} title={tr("admin.tr.showFrom", { x: f.ip })}>
            <span class="mono ip">{f.ip}</span>
            <span class="chip bad">{f.count}×</span>
            <span class="f-users ellipsis">{tr("admin.tr.tried", { names: f.usernames.map((u) => `"${u}"`).join(", ") || "—" })}</span>
            <span class="faint small" title={fullDate(f.last_at_ms)}>{agoMs(f.last_at_ms, now)}</span>
          </button>
        {/each}
      </div>
    </div>
  {/if}

  <!-- Quien pega: IPs -->
  <h2 class="section-title">{tr("admin.tr.topIps")}</h2>
  <div class="card table-card">
    <table class="list stackable">
      <thead><tr><th>IP</th><th>Tailscale</th><th>{tr("admin.tab.users")}</th><th class="r">{tr("admin.tr.requests")}</th><th class="r">{tr("admin.tr.data")}</th></tr></thead>
      <tbody>
        {#if !m}
          {#each Array(3) as _, i (i)}<tr><td colspan="5"><div class="skel line" style="width: {75 - i * 15}%"></div></td></tr>{/each}
        {:else if !m.top_ips.length}
          <tr><td colspan="5" class="none faint">{tr("admin.tr.noRequests")}</td></tr>
        {:else}
          {#each m.top_ips as ip, i (ip.ip)}
            {@const fail = failedByIp.get(ip.ip)}
            <tr class="stagger" style="--i: {i}" class:flag={!!fail}>
              <td class="lead">
                <button class="link mono" onclick={() => onfilter({ ip: ip.ip })} title={tr("admin.tr.showFromIp")}>{ip.ip}</button>
                {#if fail}<span class="chip bad" title={tr("admin.tr.failedFromIp")}>{tr("admin.tr.nFailed", { n: fail.count })}</span>{/if}
              </td>
              <td data-label="Tailscale" class="ts">
                {#if ip.tailscale_logins.length}
                  <span class="ellipsis" title={ip.tailscale_logins.join(", ")}>{ip.tailscale_logins.join(", ")}</span>
                {:else}<span class="faint">—</span>{/if}
              </td>
              <td data-label={tr("admin.tab.users")}>
                {#if ip.users.length}
                  <span class="users">
                    {#each ip.users as u (u)}<button class="chip user" onclick={() => onfilter({ user: u })} title={tr("admin.tr.showBy", { x: u })}>{u}</button>{/each}
                  </span>
                {:else}<span class="faint">{tr("admin.tr.signedOut")}</span>{/if}
              </td>
              <td data-label={tr("admin.tr.requests")} class="r num">
                <span class="share" style="--w: {(ip.requests / maxIp) * 100}%"></span>{count(ip.requests)}
              </td>
              <td data-label={tr("admin.tr.data")} class="r num">{bytes(ip.bytes_out) || "0 B"}</td>
            </tr>
          {/each}
        {/if}
      </tbody>
    </table>
  </div>

  <div class="two">
    <div>
      <h2 class="section-title">{tr("admin.tr.topUsers")}</h2>
      <div class="card table-card">
        <table class="list stackable">
          <thead><tr><th>{tr("admin.users.user")}</th><th class="r">{tr("admin.tr.requests")}</th><th class="r">{tr("admin.tr.data")}</th></tr></thead>
          <tbody>
            {#if !m}
              {#each Array(2) as _, i (i)}<tr><td colspan="3"><div class="skel line" style="width: {60 - i * 15}%"></div></td></tr>{/each}
            {:else if !m.top_users.length}
              <tr><td colspan="3" class="none faint">{tr("admin.tr.noUsers")}</td></tr>
            {:else}
              {#each m.top_users as u, i (u.user_id)}
                {@const name = u.username ?? `#${u.user_id}`}
                <tr class="stagger" style="--i: {i}">
                  <td class="lead">
                    <button class="who link" onclick={() => onfilter({ user: u.username ?? String(u.user_id) })} title={tr("admin.tr.showBy", { x: name })}>
                      <span class="avatar">{name.slice(0, 1).toUpperCase()}</span>{name}
                    </button>
                    {#if !u.username}<span class="faint small">{tr("admin.tr.deleted")}</span>{/if}
                  </td>
                  <td data-label={tr("admin.tr.requests")} class="r num">
                    <span class="share" style="--w: {(u.requests / maxUser) * 100}%"></span>{count(u.requests)}
                    <span class="faint small">{pct(u.requests, t?.requests ?? 0)}</span>
                  </td>
                  <td data-label={tr("admin.tr.data")} class="r num">{bytes(u.bytes_out) || "0 B"}</td>
                </tr>
              {/each}
            {/if}
          </tbody>
        </table>
      </div>
    </div>
    <div>
      <h2 class="section-title">{tr("admin.tr.responses")}</h2>
      <div class="card status-card">
        {#if !m}
          <div class="skel line" style="width: 100%"></div>
        {:else}
          <div class="sbar" role="img" aria-label={classes.map((c) => `${c.k}: ${c.n}`).join(", ")}>
            {#each classes as c (c.k)}
              {#if c.n}<span class="seg-{c.tone}" style="flex: {c.n}" title="{c.k}: {count(c.n)}"></span>{/if}
            {/each}
            {#if !statusTotal}<span class="seg-empty"></span>{/if}
          </div>
          <div class="classes">
            {#each classes as c (c.k)}
              <button class="cls" disabled={!c.n} onclick={() => onfilter({ status: c.k })} title={tr("admin.tr.showStatus", { x: c.k })}>
                <i class="sw seg-{c.tone}"></i><span class="mono">{c.k}</span><b>{count(c.n)}</b><span class="faint">{pct(c.n, statusTotal)}</span>
              </button>
            {/each}
          </div>
          {#if topStatuses.length}
            <div class="codes">
              {#each topStatuses as s (s.status)}
                <button class="chip" class:bad={s.status >= 500} class:warn={s.status < 500} onclick={() => onfilter({ status: String(s.status) })} title={tr("admin.tr.showStatus", { x: s.status })}>
                  {s.status} · {count(s.requests)}
                </button>
              {/each}
            </div>
          {/if}
          {#if m.dropped_events}
            <div class="dropped small">{tr("admin.tr.dropped", { n: count(m.dropped_events) })}</div>
          {/if}
        {/if}
      </div>
    </div>
  </div>

  <h2 class="section-title">{tr("admin.tr.topRoutes")}</h2>
  <div class="card table-card">
    <table class="list stackable">
      <thead><tr><th>{tr("admin.logs.route")}</th><th class="r">{tr("admin.tr.requests")}</th><th class="r">{tr("admin.tr.errors")}</th><th class="r">p95</th><th class="r">{tr("admin.tr.data")}</th></tr></thead>
      <tbody>
        {#if !m}
          {#each Array(4) as _, i (i)}<tr><td colspan="5"><div class="skel line" style="width: {80 - i * 12}%"></div></td></tr>{/each}
        {:else if !m.top_routes.length}
          <tr><td colspan="5" class="none faint">{tr("admin.tr.noRequests")}</td></tr>
        {:else}
          {#each allRoutes ? m.top_routes : m.top_routes.slice(0, 10) as r, i (r.method + r.route)}
            <tr class="stagger" style="--i: {Math.min(i, 12)}">
              <td class="lead route">
                <span class="chip meth {methodTone(r.method)}">{r.method}</span>
                <button class="link mono ellipsis" onclick={() => onfilter({ route: r.route })} title={tr("admin.tr.showTo", { x: r.route })}>{r.route}</button>
              </td>
              <td data-label={tr("admin.tr.requests")} class="r num">{count(r.requests)}</td>
              <td data-label={tr("admin.tr.errors")} class="r num" class:bad={r.errors > 0}>{r.errors ? `${count(r.errors)} · ${pct(r.errors, r.requests)}` : "0"}</td>
              <td data-label="p95" class="r num" title={tr("admin.tr.medianTip", { d: dur(r.p50_ms) })}>{dur(r.p95_ms)}</td>
              <td data-label={tr("admin.tr.data")} class="r num">{bytes(r.bytes_out) || "0 B"}</td>
            </tr>
          {/each}
        {/if}
      </tbody>
    </table>
    {#if m && m.top_routes.length > 10}
      <button class="btn ghost sm show-all" onclick={() => (allRoutes = !allRoutes)}>
        {allRoutes ? tr("admin.tr.showFewer") : tr("admin.tr.showAll", { n: m.top_routes.length })}
      </button>
    {/if}
  </div>
{/if}

<style>
  .bar {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 12px;
  }
  .upd {
    white-space: nowrap;
  }
  .small {
    font-size: 12px;
  }
  .kpis {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 12px;
  }
  .kpi {
    padding: 13px 15px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }
  .kpi.alert {
    border-color: color-mix(in srgb, var(--danger) 55%, var(--line));
  }
  .k-head {
    display: flex;
    align-items: center;
    gap: 7px;
    font-size: 11.5px;
    font-weight: 650;
    color: var(--text-3);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .k-head :global(svg) {
    color: var(--accent);
  }
  .k-n {
    font-size: 24px;
    font-weight: 700;
    letter-spacing: -0.01em;
    font-variant-numeric: tabular-nums;
  }
  .k-n.bad {
    color: var(--danger);
  }
  .k-sub {
    font-size: 12px;
    color: var(--text-3);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .k-skel {
    width: 60%;
    height: 22px;
    margin: 4px 0 6px;
  }
  .chart-card {
    margin-top: 12px;
    padding: 12px 14px 8px;
  }
  .c-head {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px 16px;
    margin-bottom: 8px;
  }
  .c-title {
    font-weight: 650;
    font-size: 14px;
    display: flex;
    gap: 6px;
    align-items: baseline;
  }
  .c-title .faint {
    font-weight: 500;
    font-size: 12.5px;
  }
  .legend {
    display: flex;
    gap: 12px;
    font-size: 12px;
    color: var(--text-2);
  }
  .legend span {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }
  .sw {
    display: inline-block;
    width: 9px;
    height: 9px;
    border-radius: 2px;
    flex: none;
  }
  .sw.main {
    background: var(--accent);
  }
  .sw.err {
    background: var(--danger);
  }
  .chart-skel {
    height: 210px;
  }
  .fails {
    margin-top: 12px;
    padding: 12px 14px;
    border-color: color-mix(in srgb, var(--danger) 50%, var(--line));
    background: color-mix(in srgb, var(--danger) 6%, var(--panel));
  }
  .f-head {
    display: flex;
    align-items: baseline;
    flex-wrap: wrap;
    gap: 4px 8px;
    font-size: 13px;
  }
  .f-head :global(svg) {
    color: var(--danger);
    align-self: center;
  }
  .f-head .muted {
    font-size: 12.5px;
  }
  .f-list {
    display: flex;
    flex-direction: column;
    margin-top: 8px;
  }
  .f-row {
    display: grid;
    grid-template-columns: minmax(120px, auto) auto minmax(0, 1fr) auto;
    align-items: center;
    gap: 10px;
    padding: 7px 8px;
    border-radius: var(--radius-sm);
    text-align: left;
  }
  .f-row:hover {
    background: var(--hover);
  }
  .f-users {
    color: var(--text-2);
  }
  .ip {
    font-weight: 600;
  }
  .table-card {
    padding: 4px 6px;
  }
  .r {
    text-align: right;
  }
  th.r {
    text-align: right;
  }
  .num {
    position: relative;
    font-family: var(--mono);
    font-size: 12.5px;
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
  .num.bad {
    color: var(--danger);
  }
  /* Barra finita debajo del numero: cuanto es del maximo de la tabla. */
  .share {
    position: absolute;
    right: 10px;
    bottom: 4px;
    width: calc(var(--w) * 0.6);
    max-width: 60%;
    height: 2px;
    border-radius: 2px;
    background: color-mix(in srgb, var(--accent) 55%, transparent);
  }
  tr.flag td {
    background: color-mix(in srgb, var(--danger) 6%, transparent);
  }
  /* Sin display:flex en la celda (la sacaria del layout de tabla): aire entre hijos con margen. */
  .lead > :global(* + *) {
    margin-left: 8px;
  }
  .lead {
    white-space: nowrap;
  }
  .link {
    font-weight: 560;
    text-align: left;
    border-radius: 4px;
    min-width: 0;
  }
  .link:hover {
    color: var(--accent);
    text-decoration: underline;
    text-underline-offset: 3px;
  }
  .ts {
    max-width: 220px;
    color: var(--text-2);
  }
  .ts span {
    display: block;
  }
  .users {
    display: inline-flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .chip.user {
    cursor: pointer;
  }
  .chip.user:hover,
  .codes .chip:hover {
    border-color: var(--line-strong);
    color: var(--text);
  }
  .who {
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }
  .avatar {
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    border-radius: 50%;
    font-weight: 700;
    font-size: 11px;
    color: var(--accent-ink);
    background: var(--primary-bg);
  }
  .route .link {
    display: inline-block;
    vertical-align: middle;
    max-width: 420px;
  }
  .meth {
    font-family: var(--mono);
    font-size: 10.5px;
    min-width: 46px;
    justify-content: center;
  }
  .none {
    text-align: center;
    padding: 22px;
  }
  .two {
    display: grid;
    grid-template-columns: minmax(0, 1.4fr) minmax(0, 1fr);
    gap: 0 14px;
  }
  .status-card {
    padding: 14px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .sbar {
    display: flex;
    gap: 2px;
    height: 10px;
    border-radius: 999px;
    overflow: hidden;
  }
  .sbar span {
    min-width: 3px;
  }
  .seg-ok {
    background: var(--ok);
  }
  .seg-muted {
    background: var(--text-3);
  }
  .seg-warn {
    background: var(--warn);
  }
  .seg-bad {
    background: var(--danger);
  }
  .seg-empty {
    flex: 1;
    background: var(--panel-2);
  }
  .classes {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 2px 12px;
  }
  .cls {
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 5px 6px;
    border-radius: var(--radius-sm);
    font-size: 12.5px;
    text-align: left;
  }
  .cls:hover:not(:disabled) {
    background: var(--hover);
  }
  .cls:disabled {
    cursor: default;
    opacity: 0.55;
  }
  .cls b {
    margin-left: auto;
    font-family: var(--mono);
    font-weight: 600;
    font-size: 12px;
  }
  .cls .faint {
    font-size: 11.5px;
    min-width: 34px;
    text-align: right;
  }
  .codes {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .codes .chip {
    cursor: pointer;
    font-family: var(--mono);
  }
  .dropped {
    color: var(--warn);
  }
  .show-all {
    margin: 6px 4px 6px;
  }
  .empty.card {
    padding: 40px 20px;
  }

  @media (max-width: 1000px) {
    .kpis {
      grid-template-columns: repeat(2, minmax(0, 1fr));
    }
    .two {
      grid-template-columns: minmax(0, 1fr);
    }
  }
  /* Celular: cada fila de tabla pasa a ser un bloque (titulo arriba, datos con su etiqueta). */
  @media (max-width: 700px) {
    .kpi {
      padding: 11px 12px;
    }
    .k-n {
      font-size: 20px;
    }
    .k-sub {
      white-space: normal;
    }
    .upd {
      display: none;
    }
    table.stackable thead {
      display: none;
    }
    table.stackable,
    table.stackable tbody {
      display: block;
    }
    table.stackable tr {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: 4px 14px;
      padding: 10px 8px;
      border-bottom: 1px solid color-mix(in srgb, var(--line) 60%, transparent);
    }
    table.stackable tr:last-child {
      border-bottom: 0;
    }
    table.stackable tr.flag {
      background: color-mix(in srgb, var(--danger) 6%, transparent);
      border-radius: var(--radius-sm);
    }
    tr.flag td {
      background: none;
    }
    table.stackable td {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      padding: 0;
      border: 0;
      text-align: left;
      max-width: 100%;
    }
    table.stackable td.lead {
      flex: 1 0 100%;
      margin-bottom: 2px;
    }
    .lead > :global(* + *) {
      margin-left: 0;
    }
    table.stackable td[data-label]::before {
      content: attr(data-label);
      font-family: var(--font);
      font-size: 11px;
      font-weight: 600;
      color: var(--text-3);
    }
    table.stackable td[colspan] {
      flex: 1;
      display: block;
    }
    .share {
      display: none;
    }
    .ts {
      max-width: 100%;
    }
    .f-row {
      grid-template-columns: auto auto 1fr;
    }
    .f-row .f-users {
      grid-column: 1 / -1;
      grid-row: 2;
    }
    .route .link {
      max-width: calc(100vw - 120px);
    }
  }
</style>
