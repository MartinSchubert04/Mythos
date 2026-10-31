<script lang="ts">
  // Admin → Logs: los requests uno por uno (filtros por usuario, IP, estado y ruta) y la
  // actividad de las cuentas (auditoria: logins, cambios de usuarios). Pagina con `before`:
  // "Load more" o solo al llegar al final de la lista.
  import { KeyRound, LogIn, LogOut, MonitorOff, RefreshCw, ShieldX, Trash2, UserCog, UserPlus, UserX, X } from "@lucide/svelte";
  import { onDestroy } from "svelte";
  import { api, errText } from "../lib/api";
  import { bytes } from "../lib/format";
  import { dur, fullDate, stamp } from "../lib/traffic";
  import type { AuditRow, LogFilters, LogRow } from "../lib/types";
  import Seg from "./Seg.svelte";
  import Select from "./Select.svelte";

  let {
    filters = $bindable(),
    mode = $bindable(),
    users = [],
  }: { filters: LogFilters; mode: "requests" | "activity"; users?: string[] } = $props();

  const PAGE = 100;

  // ---------- requests

  let rows = $state<LogRow[] | null>(null);
  let next = $state<number | null>(null);
  let busy = $state(false);
  let error = $state("");
  let ctrl: AbortController | null = null;

  async function loadLogs(more = false) {
    if (more && (busy || next == null)) return;
    ctrl?.abort();
    const c = (ctrl = new AbortController());
    busy = true;
    if (!more) error = "";
    try {
      const p = await api.logs({ ...clean(filters), limit: PAGE, before: more ? (next ?? undefined) : undefined }, c.signal);
      if (c.signal.aborted) return;
      rows = more ? [...(rows ?? []), ...p.items] : p.items;
      next = p.next_before;
    } catch (e) {
      if (!c.signal.aborted) error = errText(e);
    } finally {
      if (ctrl === c) busy = false;
    }
  }
  const clean = (f: LogFilters): LogFilters =>
    Object.fromEntries(Object.entries(f).map(([k, v]) => [k, (v ?? "").trim()]).filter(([, v]) => v)) as LogFilters;

  // Recarga al cambiar un filtro; lo que se escribe espera un poco (no un pedido por tecla).
  let deb = 0;
  let first = true;
  $effect(() => {
    const key = JSON.stringify(clean(filters));
    void key;
    if (mode !== "requests") return;
    clearTimeout(deb);
    if (first) {
      first = false;
      loadLogs();
    } else deb = window.setTimeout(() => loadLogs(), 320);
  });

  const STATUS = [
    { value: "", label: "Any status" },
    { value: "2xx", label: "2xx · OK" },
    { value: "3xx", label: "3xx · Redirect" },
    { value: "4xx", label: "4xx · Client error" },
    { value: "5xx", label: "5xx · Server error" },
    { value: "401", label: "401 · Not signed in" },
    { value: "403", label: "403 · Forbidden" },
    { value: "404", label: "404 · Not found" },
    { value: "429", label: "429 · Too many" },
    { value: "502", label: "502 · Upstream failed" },
  ];
  const statusOpts = $derived(
    filters.status && !STATUS.some((s) => s.value === filters.status) ? [...STATUS, { value: filters.status, label: filters.status }] : STATUS,
  );
  const active = $derived(Object.values(clean(filters)).length);
  const tone = (s: number) => (s >= 500 ? "bad" : s >= 400 ? "warn" : s >= 300 ? "" : "ok");
  function clearAll() {
    filters = {};
  }

  // ---------- actividad (auditoria)

  let audit = $state<AuditRow[] | null>(null);
  let aNext = $state<number | null>(null);
  let aBusy = $state(false);
  let aError = $state("");
  let event = $state("");
  let aUser = $state("");
  let actrl: AbortController | null = null;
  async function loadAudit(more = false) {
    if (more && (aBusy || aNext == null)) return;
    actrl?.abort();
    const c = (actrl = new AbortController());
    aBusy = true;
    if (!more) aError = "";
    try {
      const p = await api.audit({ event: event || undefined, user: aUser.trim() || undefined, limit: PAGE, before: more ? (aNext ?? undefined) : undefined }, c.signal);
      if (c.signal.aborted) return;
      audit = more ? [...(audit ?? []), ...p.items] : p.items;
      aNext = p.next_before;
    } catch (e) {
      if (!c.signal.aborted) aError = errText(e);
    } finally {
      if (actrl === c) aBusy = false;
    }
  }
  let aDeb = 0;
  $effect(() => {
    void [event, aUser];
    if (mode !== "activity") return;
    clearTimeout(aDeb);
    aDeb = window.setTimeout(() => loadAudit(), audit ? 320 : 0);
  });

  const EVENTS = [
    { value: "", label: "All events" },
    { value: "login_ok", label: "Signed in" },
    { value: "login_fail", label: "Failed sign-in" },
    { value: "logout", label: "Signed out" },
    { value: "session_revoked", label: "Device signed out" },
    { value: "password_changed", label: "Password changed" },
    { value: "user_created", label: "User created" },
    { value: "user_updated", label: "User edited" },
    { value: "user_disabled", label: "User disabled" },
    { value: "user_deleted", label: "User deleted" },
  ];
  type Ev = { icon: typeof LogIn; text: string; tone?: "bad" | "warn" | "ok" };
  function describe(a: AuditRow): Ev {
    const u = a.username ?? "Someone";
    switch (a.event) {
      case "login_ok":
        return { icon: LogIn, text: `${u} signed in`, tone: "ok" };
      case "login_fail":
        return { icon: ShieldX, text: `Failed sign-in as “${a.username ?? "?"}”`, tone: "bad" };
      case "logout":
        return { icon: LogOut, text: `${u} signed out` };
      case "session_revoked":
        return { icon: MonitorOff, text: `${u} signed out a device` };
      case "password_changed":
        return { icon: KeyRound, text: `${u} changed their password`, tone: "warn" };
      case "user_created":
        return { icon: UserPlus, text: `${u} created a user` };
      case "user_updated":
        return { icon: UserCog, text: `${u} edited a user` };
      case "user_disabled":
        return { icon: UserX, text: `${u} disabled a user`, tone: "warn" };
      case "user_deleted":
        return { icon: Trash2, text: `${u} deleted a user`, tone: "bad" };
      default:
        return { icon: UserCog, text: `${u}: ${a.event}` };
    }
  }

  // ---------- "infinito": cargar la pagina siguiente al ver el final

  let sentinel = $state<HTMLElement>();
  $effect(() => {
    const el = sentinel;
    if (!el) return;
    const io = new IntersectionObserver(
      (e) => {
        if (!e[0].isIntersecting) return;
        if (mode === "requests" && rows?.length) loadLogs(true);
        else if (mode === "activity" && audit?.length) loadAudit(true);
      },
      { rootMargin: "200px" },
    );
    io.observe(el);
    return () => io.disconnect();
  });
  onDestroy(() => {
    ctrl?.abort();
    actrl?.abort();
    clearTimeout(deb);
    clearTimeout(aDeb);
  });

  function showIp(ip: string) {
    filters = { ip };
    mode = "requests";
  }
</script>

<div class="bar">
  <Seg
    bind:value={mode}
    options={[
      { value: "requests", label: "Requests" },
      { value: "activity", label: "Account activity" },
    ]}
  />
  <div class="grow"></div>
  <button class="icon-btn" title="Load newest" onclick={() => (mode === "requests" ? loadLogs() : loadAudit())} disabled={mode === "requests" ? busy : aBusy}>
    <RefreshCw size={15} class={(mode === "requests" ? busy : aBusy) ? "spin" : ""} />
  </button>
</div>

{#if mode === "requests"}
  <div class="filters">
    <label class="f">
      <span>User</span>
      <input bind:value={filters.user} placeholder="Anyone (name or id)" list="log-users" autocomplete="off" spellcheck="false" />
      <datalist id="log-users">{#each users as u (u)}<option value={u}></option>{/each}</datalist>
    </label>
    <label class="f">
      <span>IP</span>
      <input bind:value={filters.ip} placeholder="Any IP" autocomplete="off" spellcheck="false" />
    </label>
    <div class="f">
      <span>Status</span>
      <Select value={filters.status ?? ""} options={statusOpts} onchange={(v) => (filters.status = v)} />
    </div>
    <label class="f wide">
      <span>Route</span>
      <input bind:value={filters.route} placeholder="Any route, e.g. /api/media" autocomplete="off" spellcheck="false" />
    </label>
    {#if active}
      <button class="btn ghost clear" onclick={clearAll}><X size={14} />Clear</button>
    {/if}
  </div>

  {#if error && !rows}
    <div class="empty card"><h3>Couldn't load the logs</h3><span>{error}</span><button class="btn" onclick={() => loadLogs()}>Retry</button></div>
  {:else}
    <div class="card table-card" class:stale={busy && !!rows}>
      <table class="list stackable logs">
        <thead><tr><th>Time</th><th>User</th><th>IP</th><th>Request</th><th>Status</th><th class="r">Took</th><th class="r">Size</th></tr></thead>
        <tbody>
          {#if !rows}
            {#each Array(6) as _, i (i)}<tr><td colspan="7"><div class="skel line" style="width: {85 - i * 9}%"></div></td></tr>{/each}
          {:else if !rows.length}
            <tr><td colspan="7" class="none faint">{active ? "No requests match these filters." : "No requests recorded yet."}</td></tr>
          {:else}
            {#each rows as r (r.id)}
              <tr>
                <td class="when mono" title={fullDate(r.ts_ms)}>{stamp(r.ts_ms)}</td>
                <td class="u">
                  {#if r.username || r.user_id}
                    {@const name = r.username ?? `#${r.user_id}`}
                    <button class="link" onclick={() => (filters = { ...filters, user: r.username ?? String(r.user_id) })} title="Only {name}">{name}</button>
                  {:else}<span class="faint">—</span>{/if}
                </td>
                <td class="ipc">
                  {#if r.ip}
                    <button class="link mono" onclick={() => (filters = { ...filters, ip: r.ip ?? "" })} title={r.tailscale_login ? `Tailscale: ${r.tailscale_login}` : `Only ${r.ip}`}>{r.ip}</button>
                    {#if r.tailscale_login}<span class="faint small ts ellipsis">{r.tailscale_login}</span>{/if}
                  {:else}<span class="faint">—</span>{/if}
                </td>
                <td class="req lead">
                  <span class="meth mono">{r.method}</span>
                  <button class="link mono ellipsis" onclick={() => (filters = { ...filters, route: r.route })} title="Only {r.route}">{r.route}</button>
                </td>
                <td class="st">
                  <button class="chip mono {tone(r.status)}" onclick={() => (filters = { ...filters, status: String(r.status) })} title="Only {r.status}">{r.status}</button>
                </td>
                <td class="r num" class:slow={r.dur_ms >= 1000}>{dur(r.dur_ms)}</td>
                <td class="r num faint">{bytes(r.bytes) || "0 B"}</td>
              </tr>
            {/each}
          {/if}
        </tbody>
      </table>
    </div>
    {#if rows?.length}
      <div class="more" bind:this={sentinel}>
        {#if next != null}
          <button class="btn" onclick={() => loadLogs(true)} disabled={busy}>{busy ? "Loading…" : "Load more"}</button>
        {:else}
          <span class="faint small">{rows.length} request{rows.length === 1 ? "" : "s"} · that's all</span>
        {/if}
        {#if error}<span class="err small">{error}</span>{/if}
      </div>
    {/if}
  {/if}
{:else}
  <div class="filters">
    <div class="f">
      <span>Event</span>
      <Select bind:value={event} options={EVENTS} />
    </div>
    <label class="f">
      <span>User</span>
      <input bind:value={aUser} placeholder="Name" list="log-users-a" autocomplete="off" spellcheck="false" />
      <datalist id="log-users-a">{#each users as u (u)}<option value={u}></option>{/each}</datalist>
    </label>
    {#if event || aUser}
      <button class="btn ghost clear" onclick={() => ((event = ""), (aUser = ""))}><X size={14} />Clear</button>
    {/if}
  </div>
  {#if aError && !audit}
    <div class="empty card"><h3>Couldn't load the activity</h3><span>{aError}</span><button class="btn" onclick={() => loadAudit()}>Retry</button></div>
  {:else}
    <div class="card feed" class:stale={aBusy && !!audit}>
      {#if !audit}
        {#each Array(5) as _, i (i)}<div class="ev"><div class="skel line" style="width: {70 - i * 8}%"></div></div>{/each}
      {:else if !audit.length}
        <div class="none faint">{event || aUser ? "Nothing matches these filters." : "No account activity yet."}</div>
      {:else}
        {#each audit as a (a.id)}
          {@const d = describe(a)}
          <div class="ev">
            <span class="ev-ico {d.tone ?? ''}"><d.icon size={15} /></span>
            <div class="ev-body">
              <div class="ev-t">{d.text}</div>
              <div class="ev-sub">
                {#if a.detail}<span class="detail">{a.detail}</span>{/if}
                {#if a.ip}<button class="link mono" onclick={() => showIp(a.ip ?? "")} title="Requests from {a.ip}">{a.ip}</button>{/if}
              </div>
            </div>
            <span class="when faint mono" title={fullDate(a.ts_ms)}>{stamp(a.ts_ms)}</span>
          </div>
        {/each}
      {/if}
    </div>
    {#if audit?.length}
      <div class="more" bind:this={sentinel}>
        {#if aNext != null}
          <button class="btn" onclick={() => loadAudit(true)} disabled={aBusy}>{aBusy ? "Loading…" : "Load more"}</button>
        {:else}
          <span class="faint small">{audit.length} event{audit.length === 1 ? "" : "s"} · that's all</span>
        {/if}
      </div>
    {/if}
  {/if}
{/if}

<style>
  .bar {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 12px;
  }
  .small {
    font-size: 12px;
  }
  .filters {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: 10px;
    margin-bottom: 12px;
  }
  .f {
    display: flex;
    flex-direction: column;
    gap: 4px;
    width: 170px;
    font-size: 11.5px;
    font-weight: 600;
    color: var(--text-3);
  }
  .f.wide {
    width: 230px;
  }
  .f input {
    height: 36px;
  }
  .clear {
    height: 36px;
  }
  .table-card {
    padding: 4px 6px;
    transition: opacity 0.15s;
  }
  .stale {
    opacity: 0.6;
  }
  table.logs td {
    padding-top: 7px;
    padding-bottom: 7px;
  }
  .r {
    text-align: right;
  }
  th.r {
    text-align: right;
  }
  .when {
    white-space: nowrap;
    color: var(--text-2);
    font-size: 12px;
  }
  .num {
    font-family: var(--mono);
    font-size: 12px;
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
  .num.slow {
    color: var(--warn);
  }
  .link {
    text-align: left;
    border-radius: 4px;
    min-width: 0;
  }
  .link:hover {
    color: var(--accent);
    text-decoration: underline;
    text-underline-offset: 3px;
  }
  .u .link {
    font-weight: 560;
  }
  .ipc {
    white-space: nowrap;
  }
  .ipc .link {
    font-size: 12px;
  }
  .ts {
    display: block;
    max-width: 160px;
  }
  /* Sin display:flex en la celda (la sacaria del layout de tabla): aire entre hijos con margen. */
  .lead > :global(* + *) {
    margin-left: 8px;
  }
  .lead {
    white-space: nowrap;
  }
  .req .link {
    display: inline-block;
    vertical-align: middle;
    max-width: 380px;
    font-size: 12px;
  }
  .meth {
    flex: none;
    font-size: 10.5px;
    font-weight: 600;
    color: var(--text-3);
    min-width: 34px;
  }
  .st .chip {
    cursor: pointer;
    font-size: 11px;
  }
  .none {
    text-align: center;
    padding: 26px;
  }
  .more {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 16px 0 4px;
  }
  .err {
    color: var(--danger);
  }
  .feed {
    padding: 4px 0;
    transition: opacity 0.15s;
  }
  .ev {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: center;
    gap: 12px;
    padding: 10px 14px;
    border-bottom: 1px solid color-mix(in srgb, var(--line) 60%, transparent);
  }
  .ev:last-child {
    border-bottom: 0;
  }
  .ev-ico {
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    border-radius: 50%;
    background: var(--panel-2);
    color: var(--text-2);
  }
  .ev-ico.ok {
    color: var(--ok);
  }
  .ev-ico.warn {
    color: var(--warn);
  }
  .ev-ico.bad {
    color: var(--danger);
    background: color-mix(in srgb, var(--danger) 12%, var(--panel-2));
  }
  .ev-t {
    font-weight: 560;
  }
  .ev-sub {
    display: flex;
    flex-wrap: wrap;
    gap: 2px 10px;
    font-size: 12px;
    color: var(--text-3);
    margin-top: 2px;
  }
  .ev-sub .link {
    font-size: 11.5px;
  }
  .detail {
    color: var(--text-2);
    overflow-wrap: anywhere;
  }
  .empty.card {
    padding: 40px 20px;
  }

  @media (max-width: 700px) {
    .f,
    .f.wide {
      width: calc(50% - 5px);
    }
    table.stackable thead {
      display: none;
    }
    table.stackable,
    table.stackable tbody {
      display: block;
    }
    /* Cada request en dos lineas: estado + ruta arriba; hora, usuario, IP y numeros abajo. */
    table.logs tr {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: 4px 12px;
      padding: 9px 6px;
      border-bottom: 1px solid color-mix(in srgb, var(--line) 60%, transparent);
    }
    table.logs tr:last-child {
      border-bottom: 0;
    }
    table.logs td {
      padding: 0;
      border: 0;
      font-size: 11.5px;
    }
    table.logs td[colspan] {
      flex: 1;
    }
    table.logs td.st {
      order: 0;
    }
    table.logs td.req {
      order: 1;
      flex: 1 1 calc(100% - 64px);
      min-width: 0;
    }
    table.logs td.when {
      order: 2;
    }
    table.logs td.u {
      order: 3;
    }
    table.logs td.ipc {
      order: 4;
    }
    table.logs td.num {
      order: 5;
    }
    .req .link {
      max-width: calc(100% - 46px);
    }
    .ts {
      display: none;
    }
    .ev {
      padding: 10px;
      gap: 10px;
    }
  }
</style>
