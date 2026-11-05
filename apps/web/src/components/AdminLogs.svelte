<script lang="ts">
  // Admin → Logs: los requests uno por uno (filtros por usuario, IP, estado y ruta) y la
  // actividad de las cuentas (auditoria: logins, cambios de usuarios). Pagina con `before`:
  // "Load more" o solo al llegar al final de la lista.
  import { KeyRound, LogIn, LogOut, MonitorOff, RefreshCw, ShieldX, Trash2, UserCog, UserPlus, UserX, X } from "@lucide/svelte";
  import { onDestroy } from "svelte";
  import { api, errText } from "../lib/api";
  import { bytes } from "../lib/format";
  import { t } from "../lib/i18n.svelte";
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

  // Las etiquetas son getters: la lista es fija y el texto sigue al idioma de la interfaz.
  const STATUS: { value: string; label: string }[] = [
    { value: "", get label() { return t("admin.logs.st.any"); } },
    { value: "2xx", get label() { return t("admin.logs.st.2xx"); } },
    { value: "3xx", get label() { return t("admin.logs.st.3xx"); } },
    { value: "4xx", get label() { return t("admin.logs.st.4xx"); } },
    { value: "5xx", get label() { return t("admin.logs.st.5xx"); } },
    { value: "401", get label() { return t("admin.logs.st.401"); } },
    { value: "403", get label() { return t("admin.logs.st.403"); } },
    { value: "404", get label() { return t("admin.logs.st.404"); } },
    { value: "429", get label() { return t("admin.logs.st.429"); } },
    { value: "502", get label() { return t("admin.logs.st.502"); } },
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

  const EVENTS: { value: string; label: string }[] = [
    { value: "", get label() { return t("admin.logs.ev.all"); } },
    { value: "login_ok", get label() { return t("admin.logs.ev.login_ok"); } },
    { value: "login_fail", get label() { return t("admin.logs.ev.login_fail"); } },
    { value: "logout", get label() { return t("admin.logs.ev.logout"); } },
    { value: "session_revoked", get label() { return t("admin.logs.ev.session_revoked"); } },
    { value: "password_changed", get label() { return t("admin.logs.ev.password_changed"); } },
    { value: "user_created", get label() { return t("admin.logs.ev.user_created"); } },
    { value: "user_updated", get label() { return t("admin.logs.ev.user_updated"); } },
    { value: "user_disabled", get label() { return t("admin.logs.ev.user_disabled"); } },
    { value: "user_deleted", get label() { return t("admin.logs.ev.user_deleted"); } },
  ];
  type Ev = { icon: typeof LogIn; text: string; tone?: "bad" | "warn" | "ok" };
  function describe(a: AuditRow): Ev {
    const u = a.username ?? t("admin.logs.someone");
    switch (a.event) {
      case "login_ok":
        return { icon: LogIn, text: t("admin.logs.d.login_ok", { u }), tone: "ok" };
      case "login_fail":
        return { icon: ShieldX, text: t("admin.logs.d.login_fail", { u: a.username ?? "?" }), tone: "bad" };
      case "logout":
        return { icon: LogOut, text: t("admin.logs.d.logout", { u }) };
      case "session_revoked":
        return { icon: MonitorOff, text: t("admin.logs.d.session_revoked", { u }) };
      case "password_changed":
        return { icon: KeyRound, text: t("admin.logs.d.password_changed", { u }), tone: "warn" };
      case "user_created":
        return { icon: UserPlus, text: t("admin.logs.d.user_created", { u }) };
      case "user_updated":
        return { icon: UserCog, text: t("admin.logs.d.user_updated", { u }) };
      case "user_disabled":
        return { icon: UserX, text: t("admin.logs.d.user_disabled", { u }), tone: "warn" };
      case "user_deleted":
        return { icon: Trash2, text: t("admin.logs.d.user_deleted", { u }), tone: "bad" };
      case "user_renamed":
        return { icon: UserCog, text: t("admin.logs.d.user_renamed", { u }) };
      case "avatar_added":
        return { icon: UserCog, text: t("admin.logs.d.avatar_added", { u }) };
      case "avatar_removed":
        return { icon: UserCog, text: t("admin.logs.d.avatar_removed", { u }) };
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
      { value: "requests", label: t("admin.logs.requests") },
      { value: "activity", label: t("admin.logs.activity") },
    ]}
  />
  <div class="grow"></div>
  <button class="icon-btn" title={t("admin.logs.loadNewest")} onclick={() => (mode === "requests" ? loadLogs() : loadAudit())} disabled={mode === "requests" ? busy : aBusy}>
    <RefreshCw size={15} class={(mode === "requests" ? busy : aBusy) ? "spin" : ""} />
  </button>
</div>

{#if mode === "requests"}
  <div class="filters">
    <label class="f">
      <span>{t("admin.users.user")}</span>
      <input bind:value={filters.user} placeholder={t("admin.logs.anyone")} list="log-users" autocomplete="off" spellcheck="false" />
      <datalist id="log-users">{#each users as u (u)}<option value={u}></option>{/each}</datalist>
    </label>
    <label class="f">
      <span>IP</span>
      <input bind:value={filters.ip} placeholder={t("admin.logs.anyIp")} autocomplete="off" spellcheck="false" />
    </label>
    <div class="f">
      <span>{t("admin.logs.status")}</span>
      <Select value={filters.status ?? ""} options={statusOpts} onchange={(v) => (filters.status = v)} />
    </div>
    <label class="f wide">
      <span>{t("admin.logs.route")}</span>
      <input bind:value={filters.route} placeholder={t("admin.logs.anyRoute")} autocomplete="off" spellcheck="false" />
    </label>
    {#if active}
      <button class="btn ghost clear" onclick={clearAll}><X size={14} />{t("admin.clear")}</button>
    {/if}
  </div>

  {#if error && !rows}
    <div class="empty card"><h3>{t("admin.logs.loadError")}</h3><span>{error}</span><button class="btn" onclick={() => loadLogs()}>{t("common.retry")}</button></div>
  {:else}
    <div class="card table-card" class:stale={busy && !!rows}>
      <table class="list stackable logs">
        <thead><tr><th>{t("admin.logs.time")}</th><th>{t("admin.users.user")}</th><th>IP</th><th>{t("admin.logs.request")}</th><th>{t("admin.logs.status")}</th><th class="r">{t("admin.logs.took")}</th><th class="r">{t("admin.logs.size")}</th></tr></thead>
        <tbody>
          {#if !rows}
            {#each Array(6) as _, i (i)}<tr><td colspan="7"><div class="skel line" style="width: {85 - i * 9}%"></div></td></tr>{/each}
          {:else if !rows.length}
            <tr><td colspan="7" class="none faint">{active ? t("admin.logs.noMatch") : t("admin.logs.noRequests")}</td></tr>
          {:else}
            {#each rows as r (r.id)}
              <tr>
                <td class="when mono" title={fullDate(r.ts_ms)}>{stamp(r.ts_ms)}</td>
                <td class="u">
                  {#if r.username || r.user_id}
                    {@const name = r.username ?? `#${r.user_id}`}
                    <button class="link" onclick={() => (filters = { ...filters, user: r.username ?? String(r.user_id) })} title={t("admin.logs.only", { x: name })}>{name}</button>
                  {:else}<span class="faint">—</span>{/if}
                </td>
                <td class="ipc">
                  {#if r.ip}
                    <button class="link mono" onclick={() => (filters = { ...filters, ip: r.ip ?? "" })} title={r.tailscale_login ? `Tailscale: ${r.tailscale_login}` : t("admin.logs.only", { x: r.ip })}>{r.ip}</button>
                    {#if r.tailscale_login}<span class="faint small ts ellipsis">{r.tailscale_login}</span>{/if}
                  {:else}<span class="faint">—</span>{/if}
                </td>
                <td class="req lead">
                  <span class="meth mono">{r.method}</span>
                  <button class="link mono ellipsis" onclick={() => (filters = { ...filters, route: r.route })} title={t("admin.logs.only", { x: r.route })}>{r.route}</button>
                </td>
                <td class="st">
                  <button class="chip mono {tone(r.status)}" onclick={() => (filters = { ...filters, status: String(r.status) })} title={t("admin.logs.only", { x: r.status })}>{r.status}</button>
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
          <button class="btn" onclick={() => loadLogs(true)} disabled={busy}>{busy ? t("common.loading") : t("admin.logs.loadMore")}</button>
        {:else}
          <span class="faint small">{t("admin.logs.allRequests", { n: rows.length })}</span>
        {/if}
        {#if error}<span class="err small">{error}</span>{/if}
      </div>
    {/if}
  {/if}
{:else}
  <div class="filters">
    <div class="f">
      <span>{t("admin.logs.event")}</span>
      <Select bind:value={event} options={EVENTS} />
    </div>
    <label class="f">
      <span>{t("admin.users.user")}</span>
      <input bind:value={aUser} placeholder={t("admin.logs.name")} list="log-users-a" autocomplete="off" spellcheck="false" />
      <datalist id="log-users-a">{#each users as u (u)}<option value={u}></option>{/each}</datalist>
    </label>
    {#if event || aUser}
      <button class="btn ghost clear" onclick={() => ((event = ""), (aUser = ""))}><X size={14} />{t("admin.clear")}</button>
    {/if}
  </div>
  {#if aError && !audit}
    <div class="empty card"><h3>{t("admin.logs.activityError")}</h3><span>{aError}</span><button class="btn" onclick={() => loadAudit()}>{t("common.retry")}</button></div>
  {:else}
    <div class="card feed" class:stale={aBusy && !!audit}>
      {#if !audit}
        {#each Array(5) as _, i (i)}<div class="ev"><div class="skel line" style="width: {70 - i * 8}%"></div></div>{/each}
      {:else if !audit.length}
        <div class="none faint">{event || aUser ? t("admin.logs.nothingMatches") : t("admin.logs.noActivity")}</div>
      {:else}
        {#each audit as a (a.id)}
          {@const d = describe(a)}
          <div class="ev">
            <span class="ev-ico {d.tone ?? ''}"><d.icon size={15} /></span>
            <div class="ev-body">
              <div class="ev-t">{d.text}</div>
              <div class="ev-sub">
                {#if a.detail}<span class="detail">{a.detail}</span>{/if}
                {#if a.ip}<button class="link mono" onclick={() => showIp(a.ip ?? "")} title={t("admin.logs.requestsFrom", { ip: a.ip })}>{a.ip}</button>{/if}
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
          <button class="btn" onclick={() => loadAudit(true)} disabled={aBusy}>{aBusy ? t("common.loading") : t("admin.logs.loadMore")}</button>
        {:else}
          <span class="faint small">{t("admin.logs.allEvents", { n: audit.length })}</span>
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
