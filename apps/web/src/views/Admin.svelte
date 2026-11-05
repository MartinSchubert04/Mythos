<script lang="ts">
  // Admin, en pestañas: Overview (cache, disco, que se esta viendo y torrents en vivo), Traffic
  // (cuanto se le pega al server, por IP y por usuario), Logs (requests y actividad de las
  // cuentas) y Users. La pestaña va en la URL (#/admin?tab=logs).
  import { HardDrive, KeyRound, MonitorSmartphone, PencilLine, Plus, RefreshCw, ShieldCheck, Trash2, UserPlus, Users } from "@lucide/svelte";
  import { onDestroy } from "svelte";
  import AdminAvatars from "../components/AdminAvatars.svelte";
  import AdminLogs from "../components/AdminLogs.svelte";
  import AdminTraffic from "../components/AdminTraffic.svelte";
  import Avatar from "../components/Avatar.svelte";
  import Devices from "../components/Devices.svelte";
  import Drawer from "../components/Drawer.svelte";
  import Seg from "../components/Seg.svelte";
  import Switch from "../components/Switch.svelte";
  import { api, errText } from "../lib/api";
  import { ask, prompt } from "../lib/confirm.svelte";
  import { bytes, relTime, speed } from "../lib/format";
  import { rise } from "../lib/motion";
  import { route, setQuery } from "../lib/router.svelte";
  import { keepScroll } from "../lib/scroll";
  import { session } from "../lib/session.svelte";
  import { has, t as tr } from "../lib/i18n.svelte";
  import { attempt, toast } from "../lib/toast.svelte";
  import type { AdminStats, LogFilters, TorrentStat, User } from "../lib/types";

  type Tab = "overview" | "traffic" | "logs" | "users" | "avatars";
  /** El estado de librqbit (initializing, live, paused, error) en el idioma de la interfaz. */
  const torrentState = (s: string) => {
    const k = `admin.torrents.state.${s}`;
    return has(k) ? tr(k) : s;
  };
  const tabOpt = (value: Tab) => ({
    value,
    get label() {
      return tr(`admin.tab.${value}` as const);
    },
  });
  const TABS: { value: Tab; label: string }[] = [tabOpt("overview"), tabOpt("traffic"), tabOpt("logs"), tabOpt("users"), tabOpt("avatars")];
  const fromUrl = (): Tab => {
    const t = route.query.get("tab");
    return TABS.some((x) => x.value === t) ? (t as Tab) : "overview";
  };
  let tab = $state<Tab>(fromUrl());
  let page = $state<HTMLDivElement>();
  function pickTab(t: Tab) {
    tab = t;
    setQuery(t === "overview" ? {} : { tab: t });
    page?.scrollTo({ top: 0 });
  }

  // Los filtros de los logs viven aca: Traffic los llena al tocar una IP, un usuario o una ruta.
  let filters = $state<LogFilters>({});
  let logMode = $state<"requests" | "activity">("requests");
  function openLogs(f: LogFilters) {
    filters = f;
    logMode = "requests";
    pickTab("logs");
  }

  let users = $state<User[] | null>(null);
  let torrents = $state<TorrentStat[] | null>(null);
  let stats = $state<AdminStats | null>(null);
  let error = $state("");

  let creating = $state(false);
  let nu = $state({ username: "", password: "", is_admin: false });
  let saving = $state(false);

  function loadUsers() {
    api
      .users()
      .then((l) => (users = l))
      .catch((e) => (error = errText(e)));
  }
  // Sin permisos (alguien que escribio #/admin a mano) no se pide nada: el server diria 403.
  const isAdmin = !!session.user?.is_admin;
  if (isAdmin) loadUsers();

  // Torrents y caché cada 2 s, solo en Overview y con la pestaña del navegador visible (cada
  // pedido tambien queda en los logs: no llenarlos de ruido desde las otras pestañas).
  let timer = 0;
  let alive = true;
  async function tick() {
    if (!alive) return;
    if (!document.hidden && (tab === "overview" || !stats)) {
      const [t, s] = await Promise.allSettled([api.torrents(), api.stats()]);
      if (t.status === "fulfilled") torrents = t.value;
      if (s.status === "fulfilled") stats = s.value;
    }
    if (alive) timer = window.setTimeout(tick, 2000);
  }
  if (isAdmin) tick();
  onDestroy(() => {
    alive = false;
    clearTimeout(timer);
  });

  async function create() {
    if (!nu.username.trim() || !nu.password) return;
    saving = true;
    try {
      const u = await api.createUser({ ...nu, username: nu.username.trim() });
      users = [...(users ?? []), u];
      toast(tr("admin.users.created", { name: u.username }));
      creating = false;
      nu = { username: "", password: "", is_admin: false };
    } catch (e) {
      toast(errText(e), true);
    } finally {
      saving = false;
    }
  }

  async function patch(u: User, p: { username?: string; disabled?: boolean; is_admin?: boolean; password?: string }, ok?: string) {
    try {
      const r = await api.patchUser(u.id, p);
      users = users?.map((x) => (x.id === u.id ? r : x)) ?? null;
      if (ok) toast(ok);
    } catch (e) {
      toast(errText(e), true);
      loadUsers();
    }
  }

  /** Las sesiones abiertas de ese usuario (Drawer): cerrar una, p. ej. una TV que no vence. */
  let devicesOf = $state<User | null>(null);

  async function rename(u: User) {
    const name = await prompt({ title: tr("admin.users.renameTitle", { name: u.username }), placeholder: tr("admin.users.newName"), value: u.username, confirm: tr("admin.users.rename") });
    if (!name || name.trim() === u.username) return;
    await patch(u, { username: name.trim() }, tr("admin.users.renamed", { name: name.trim() }));
    if (u.id === session.user?.id) session.user = users?.find((x) => x.id === u.id) ?? session.user;
  }

  async function resetPw(u: User) {
    const pw = await prompt({ title: tr("admin.users.pwTitle", { name: u.username }), placeholder: tr("admin.users.newPw"), secret: true, confirm: tr("admin.users.setPw") });
    if (pw) patch(u, { password: pw }, tr("admin.users.pwChanged"));
  }

  async function remove(u: User) {
    const ok = await ask({
      title: tr("admin.users.deleteTitle", { name: u.username }),
      body: tr("admin.users.deleteBody"),
      confirm: tr("admin.users.delete"),
      danger: true,
    });
    if (ok && (await attempt(() => api.deleteUser(u.id), tr("admin.users.deleted", { name: u.username })))) users = users?.filter((x) => x.id !== u.id) ?? null;
  }

  async function dropTorrent(t: TorrentStat) {
    const ok = await ask({
      title: tr("admin.torrents.removeTitle"),
      body: `${t.name}\n${tr("admin.torrents.removeBody")}`,
      confirm: tr("common.remove"),
      danger: true,
    });
    if (ok && (await attempt(() => api.deleteTorrent(t.info_hash, true), tr("admin.torrents.removed"))))
      torrents = torrents?.filter((x) => x.info_hash !== t.info_hash) ?? null;
  }

  // Nombres de lo que se esta viendo (el server manda solo el id).
  let names = $state<Record<string, string>>({});
  $effect(() => {
    for (const w of stats?.watching ?? []) {
      const id = w.meta_id;
      if (!id || names[id] !== undefined) continue;
      names[id] = "";
      const [base, s, e] = id.split(":");
      api
        .meta(s ? "series" : "movie", base)
        .then((m) => (names[id] = s ? `${m.name} · S${s.padStart(2, "0")}E${e.padStart(2, "0")}` : m.name))
        .catch(() => (names[id] = id));
    }
  });
  const ago = (unix?: number | null) => (unix ? relTime(new Date(unix * 1000).toISOString()) : "");

  const cachePct = $derived(stats && stats.cache_limit_bytes ? Math.min(100, (stats.cache_bytes / stats.cache_limit_bytes) * 100) : 0);
</script>

<div class="page" use:keepScroll={"admin"} bind:this={page}>
  <div class="page-head">
    <h1>Admin</h1>
    <div class="grow"></div>
    {#if tab === "users" && isAdmin}
      <button class="btn primary" onclick={() => (creating = true)}><UserPlus size={15} />{tr("admin.users.new")}</button>
    {/if}
  </div>
  {#if !isAdmin}
  <div class="empty"><ShieldCheck size={28} /><h3>{tr("admin.only")}</h3><span>{tr("admin.onlyHint")}</span></div>
  {:else}
  <nav class="tabs" aria-label={tr("admin.sections")}><Seg value={tab} options={TABS} onchange={pickTab} /></nav>

  {#key tab}
  <div class="tab" in:rise={{ y: 6 }}>
  {#if tab === "traffic"}
    <AdminTraffic onfilter={openLogs} />
  {:else if tab === "avatars"}
    <AdminAvatars />
  {:else if tab === "logs"}
    <AdminLogs bind:filters bind:mode={logMode} users={users?.map((u) => u.username) ?? []} />
  {:else if tab === "overview"}

  <!-- Uso de cache y disco -->
  <div class="stats-grid">
    <div class="card stat">
      <div class="stat-head"><HardDrive size={15} />{tr("admin.cache")}</div>
      {#if stats}
        <div class="stat-n">{bytes(stats.cache_bytes)} <span class="faint">{tr("admin.cacheOf", { size: bytes(stats.cache_limit_bytes) })}</span></div>
        <div class="progress big"><div class="progress-fill bar-fill" class:warn={cachePct > 85} style="width: {cachePct}%"></div></div>
        <div class="faint small">{tr("admin.diskFree", { size: bytes(stats.disk_free_bytes) })}</div>
      {:else}
        <div class="skel line" style="width: 60%; margin-top: 10px"></div>
        <div class="skel line" style="width: 100%; margin-top: 12px"></div>
      {/if}
    </div>
    <div class="card stat">
      <div class="stat-head"><RefreshCw size={15} />{tr("admin.playingNow")}</div>
      <div class="stat-n">{stats?.active_sessions ?? "–"}</div>
      {#if stats?.watching?.length}
        <div class="watching">
          {#each stats.watching as w (w.username + (w.session_id ?? ""))}
            <div class="w ellipsis" title="{w.username}: {names[w.meta_id ?? ""] || w.meta_id}">
              <b>{w.username}</b> · {names[w.meta_id ?? ""] || w.meta_id || "…"} <span class="faint">{ago(w.started_at)}</span>
            </div>
          {/each}
        </div>
      {:else}
        <div class="faint small">{tr("admin.openSessions")}</div>
      {/if}
    </div>
    <button class="card stat link-card" onclick={() => pickTab("users")} title={tr("admin.manageUsers")}>
      <div class="stat-head"><Users size={15} />{tr("admin.tab.users")}</div>
      <div class="stat-n">{stats?.users ?? users?.length ?? "–"}</div>
      <div class="faint small">{tr("admin.admins", { n: users?.filter((u) => u.is_admin).length ?? 0 })}</div>
    </button>
  </div>

  <h2 class="section-title">Torrents <span class="live"><span class="dot"></span>{tr("admin.live")}</span></h2>
  <div class="card table-card">
    <table class="list torrents">
      <thead><tr><th>{tr("admin.torrents.name")}</th><th>{tr("admin.torrents.progress")}</th><th>{tr("admin.torrents.down")}</th><th>{tr("admin.torrents.up")}</th><th>Peers</th><th>{tr("admin.lastUsed")}</th><th></th></tr></thead>
      <tbody>
        {#if !torrents}
          {#each Array(2) as _, i (i)}<tr><td colspan="7"><div class="skel line" style="width: {80 - i * 20}%"></div></td></tr>{/each}
        {:else if !torrents.length}
          <tr><td colspan="7" class="faint none">{tr("admin.torrents.none")}</td></tr>
        {:else}
          {#each torrents as t (t.info_hash)}
            {@const pct = t.size_bytes ? (t.downloaded_bytes / t.size_bytes) * 100 : 0}
            <tr>
              <td class="tname"><span class="ellipsis" title={t.name}>{t.name}</span><span class="faint small">{bytes(t.size_bytes)} · {torrentState(t.state)}</span></td>
              <td class="tprog">
                <div class="progress"><div class="progress-fill" style="width: {pct}%"></div></div>
                <span class="faint small">{pct.toFixed(0)}%</span>
              </td>
              <td class="num down" class:active={t.down_bps > 0}>{speed(t.down_bps)}</td>
              <td class="num faint up">{speed(t.up_bps)}</td>
              <td class="num peers">{t.peers}</td>
              <td class="faint last">{relTime(t.last_used_at)}</td>
              <td class="acts"><button class="icon-btn del" title={tr("admin.torrents.removeTip")} onclick={() => dropTorrent(t)}><Trash2 size={15} /></button></td>
            </tr>
          {/each}
        {/if}
      </tbody>
    </table>
  </div>
  {:else}
  {#if error}
    <div class="empty"><h3>{tr("admin.users.loadError")}</h3><span>{error}</span></div>
  {:else}
    <div class="card table-card">
      <table class="list users">
        <thead><tr><th>{tr("admin.users.user")}</th><th>{tr("admin.users.role")}</th><th>{tr("admin.users.createdCol")}</th><th>{tr("admin.users.enabled")}</th><th></th></tr></thead>
        <tbody>
          {#if !users}
            {#each Array(3) as _, i (i)}<tr><td colspan="5"><div class="skel line" style="width: {70 - i * 12}%"></div></td></tr>{/each}
          {:else}
            {#each users as u, i (u.id)}
              {@const me = u.id === session.user?.id}
              <tr class="stagger" style="--i: {i}" class:off={u.disabled}>
                <td class="u-who">
                  <span class="who"><Avatar name={u.username} id={u.avatar_id} size={26} />{u.username}{#if me}<span class="chip">{tr("admin.users.you")}</span>{/if}</span>
                </td>
                <td class="u-role">
                  <button class="chip role" class:accent={u.is_admin} disabled={me} title={me ? "" : tr("admin.users.clickToChange")} onclick={() => patch(u, { is_admin: !u.is_admin }, u.is_admin ? tr("admin.users.noLongerAdmin") : tr("admin.users.nowAdmin"))}>
                    {u.is_admin ? "Admin" : tr("admin.users.user")}
                  </button>
                </td>
                <td class="faint u-created">{relTime(u.created_at)}</td>
                <td class="u-on"><Switch checked={!u.disabled} disabled={me} onchange={(v) => patch(u, { disabled: !v }, v ? tr("admin.users.enabledToast") : tr("admin.users.disabledToast"))} /></td>
                <td class="acts">
                  <button class="icon-btn" title={tr("admin.users.devices")} onclick={() => (devicesOf = u)}><MonitorSmartphone size={15} /></button>
                  <button class="icon-btn" title={tr("admin.users.rename")} onclick={() => rename(u)}><PencilLine size={15} /></button>
                  <button class="icon-btn" title={tr("admin.users.resetPw")} onclick={() => resetPw(u)}><KeyRound size={15} /></button>
                  <button class="icon-btn del" title={tr("admin.users.delete")} disabled={me} onclick={() => remove(u)}><Trash2 size={15} /></button>
                </td>
              </tr>
            {/each}
          {/if}
        </tbody>
      </table>
    </div>
  {/if}
  {/if}
  </div>
  {/key}
  {/if}
</div>

{#if creating}
  <Drawer title={tr("admin.users.new")} onclose={() => (creating = false)} onsave={create}>
    <label class="field">
      {tr("admin.users.username")}
      <!-- svelte-ignore a11y_autofocus -->
      <input bind:value={nu.username} autofocus autocomplete="off" spellcheck="false" />
    </label>
    <label class="field">
      {tr("admin.users.password")}
      <input type="password" bind:value={nu.password} autocomplete="new-password" />
      <span class="hint">{tr("admin.users.pwHint")}</span>
    </label>
    <Switch bind:checked={nu.is_admin}>{tr("admin.users.adminSwitch")}</Switch>
    {#snippet footer()}
      <button class="btn" onclick={() => (creating = false)}>{tr("common.cancel")}</button>
      <button class="btn primary" disabled={saving || !nu.username.trim() || !nu.password} onclick={create}><Plus size={15} />{tr("admin.users.create")}</button>
    {/snippet}
  </Drawer>
{/if}

{#if devicesOf}
  <Drawer title={tr("admin.users.devicesOf", { name: devicesOf.username })} onclose={() => (devicesOf = null)}>
    <p class="muted">{tr("admin.users.devicesHint")}</p>
    {#key devicesOf.id}<Devices user={devicesOf} />{/key}
  </Drawer>
{/if}

<style>
  .page {
    padding: 26px var(--gutter) 40px;
    max-width: 1600px;
  }
  .tabs {
    display: flex;
    margin: -6px 0 18px;
  }
  .link-card {
    text-align: left;
    transition: border-color 0.15s, transform 0.2s cubic-bezier(0.2, 0.8, 0.2, 1);
  }
  .link-card:hover {
    border-color: var(--line-strong);
    transform: translateY(-1px);
  }
  .stats-grid {
    display: grid;
    grid-template-columns: 2fr 1fr 1fr;
    gap: 12px;
  }
  .stat {
    padding: 14px 16px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .stat-head {
    display: flex;
    align-items: center;
    gap: 7px;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-3);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .stat-head :global(svg) {
    color: var(--accent);
  }
  .stat-n {
    font-size: 22px;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
  }
  .watching {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: 12px;
    color: var(--text-2);
    min-width: 0;
  }
  .stat-n .faint {
    font-size: 13px;
    font-weight: 500;
  }
  .progress.big {
    height: 8px;
  }
  .bar-fill {
    transition: width 0.6s cubic-bezier(0.2, 0.8, 0.2, 1);
  }
  .bar-fill.warn {
    background: var(--warn);
  }
  .small {
    font-size: 12px;
  }
  .table-card {
    padding: 4px 6px;
    overflow-x: auto;
  }
  tr.off td {
    opacity: 0.55;
  }
  .who {
    display: inline-flex;
    align-items: center;
    gap: 9px;
    font-weight: 560;
  }
  .role {
    cursor: pointer;
    transition: border-color 0.15s;
  }
  .role:hover:not(:disabled) {
    border-color: var(--line-strong);
  }
  .role:disabled {
    cursor: default;
  }
  .acts {
    text-align: right;
    white-space: nowrap;
  }
  .del:hover {
    color: var(--danger);
  }
  .icon-btn:disabled {
    opacity: 0.3;
    pointer-events: none;
  }
  .tname {
    max-width: 360px;
  }
  .tname span {
    display: block;
  }
  .tprog {
    min-width: 120px;
  }
  .tprog .progress {
    margin-bottom: 3px;
  }
  .num {
    font-family: var(--mono);
    font-size: 12.5px;
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
  .num.active {
    color: var(--ok);
  }
  .none {
    text-align: center;
    padding: 24px;
  }
  .live {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    margin-left: 6px;
    color: var(--ok);
    font-size: 10.5px;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--ok);
    animation: pulse 1.6s ease-in-out infinite;
  }
  @keyframes pulse {
    50% {
      opacity: 0.3;
      transform: scale(0.8);
    }
  }
  /* Celular: las tablas pasan a tarjetas (una por fila), sin scroll de costado. */
  @media (max-width: 700px) {
    .table-card {
      overflow-x: visible;
    }
    .list thead {
      display: none;
    }
    .list,
    .list tbody {
      display: block;
      width: 100%;
    }
    .list tr {
      display: grid;
      align-items: center;
      gap: 6px 12px;
      padding: 12px 10px;
      border-bottom: var(--border) solid var(--line);
    }
    .list tr:last-child {
      border-bottom: 0;
    }
    .list td {
      display: block;
      padding: 0;
      border: 0;
      min-width: 0;
    }
    .list td[colspan] {
      grid-column: 1 / -1;
    }
    .torrents tr {
      grid-template-columns: auto auto auto 1fr auto;
      grid-template-areas:
        "name name name name acts"
        "prog prog prog prog prog"
        "down up peers last last";
    }
    .tname {
      grid-area: name;
      max-width: none;
    }
    .list td.tprog {
      grid-area: prog;
      display: flex;
      align-items: center;
      gap: 10px;
      min-width: 0;
    }
    .tprog .progress {
      flex: 1;
      margin: 0;
    }
    .torrents .acts {
      grid-area: acts;
      align-self: start;
    }
    .down {
      grid-area: down;
    }
    .up {
      grid-area: up;
    }
    .peers {
      grid-area: peers;
    }
    .last {
      grid-area: last;
      text-align: right;
      font-size: 12px;
    }
    .down::before {
      content: "↓ ";
    }
    .up::before {
      content: "↑ ";
    }
    .peers::after {
      content: " peers";
    }
    .users tr {
      grid-template-columns: auto 1fr auto;
      grid-template-areas:
        "who who on"
        "role created acts";
    }
    .u-who {
      grid-area: who;
      min-width: 0;
    }
    .u-role {
      grid-area: role;
    }
    .u-created {
      grid-area: created;
      font-size: 12px;
    }
    .u-on {
      grid-area: on;
      justify-self: end;
    }
    .users .acts {
      grid-area: acts;
    }
    .tabs > :global(*) {
      flex: 1;
    }
    /* Cinco pestañas en 358 px: en español ("Registros", "Avatares") no entraban con el relleno de
       siempre y la ultima quedaba cortada contra el borde. */
    .tabs :global(.seg2 button) {
      padding: 0 4px;
    }
    .stats-grid {
      grid-template-columns: 1fr 1fr;
    }
    .stats-grid > :first-child {
      grid-column: 1 / -1;
    }
  }
</style>
