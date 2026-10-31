<script lang="ts">
  // Admin: usuarios, torrents activos (velocidad en vivo, solo mientras la vista esta abierta) y
  // el uso de cache y disco.
  import { HardDrive, KeyRound, Plus, RefreshCw, Trash2, UserPlus, Users } from "@lucide/svelte";
  import { onDestroy } from "svelte";
  import Drawer from "../components/Drawer.svelte";
  import Switch from "../components/Switch.svelte";
  import { api, errText } from "../lib/api";
  import { ask, prompt } from "../lib/confirm.svelte";
  import { bytes, relTime, speed } from "../lib/format";
  import { keepScroll } from "../lib/scroll";
  import { session } from "../lib/session.svelte";
  import { attempt, toast } from "../lib/toast.svelte";
  import type { AdminStats, TorrentStat, User } from "../lib/types";

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
  loadUsers();

  // Torrents y caché cada 2 s, solo con la vista abierta y la pestaña visible.
  let timer = 0;
  let alive = true;
  async function tick() {
    if (!alive) return;
    if (!document.hidden) {
      const [t, s] = await Promise.allSettled([api.torrents(), api.stats()]);
      if (t.status === "fulfilled") torrents = t.value;
      if (s.status === "fulfilled") stats = s.value;
    }
    if (alive) timer = window.setTimeout(tick, 2000);
  }
  tick();
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
      toast(`User ${u.username} created`);
      creating = false;
      nu = { username: "", password: "", is_admin: false };
    } catch (e) {
      toast(errText(e), true);
    } finally {
      saving = false;
    }
  }

  async function patch(u: User, p: { disabled?: boolean; is_admin?: boolean; password?: string }, ok?: string) {
    try {
      const r = await api.patchUser(u.id, p);
      users = users?.map((x) => (x.id === u.id ? r : x)) ?? null;
      if (ok) toast(ok);
    } catch (e) {
      toast(errText(e), true);
      loadUsers();
    }
  }

  async function resetPw(u: User) {
    const pw = await prompt({ title: `New password for ${u.username}`, placeholder: "New password", secret: true, confirm: "Set password" });
    if (pw) patch(u, { password: pw }, "Password changed");
  }

  async function remove(u: User) {
    const ok = await ask({
      title: `Delete ${u.username}?`,
      body: "Their progress, library and settings are deleted too. This can't be undone.",
      confirm: "Delete user",
      danger: true,
    });
    if (ok && (await attempt(() => api.deleteUser(u.id), `${u.username} deleted`))) users = users?.filter((x) => x.id !== u.id) ?? null;
  }

  async function dropTorrent(t: TorrentStat) {
    const ok = await ask({
      title: "Remove this torrent?",
      body: `${t.name}\nIts downloaded files are deleted from the cache.`,
      confirm: "Remove",
      danger: true,
    });
    if (ok && (await attempt(() => api.deleteTorrent(t.info_hash, true), "Torrent removed")))
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

<div class="page" use:keepScroll={"admin"}>
  <div class="page-head">
    <h1>Admin</h1>
    <div class="grow"></div>
    <button class="btn primary" onclick={() => (creating = true)}><UserPlus size={15} />New user</button>
  </div>

  <!-- Uso de cache y disco -->
  <div class="stats-grid">
    <div class="card stat">
      <div class="stat-head"><HardDrive size={15} />Cache</div>
      {#if stats}
        <div class="stat-n">{bytes(stats.cache_bytes)} <span class="faint">of {bytes(stats.cache_limit_bytes)}</span></div>
        <div class="progress big"><div class="progress-fill bar-fill" class:warn={cachePct > 85} style="width: {cachePct}%"></div></div>
        <div class="faint small">{bytes(stats.disk_free_bytes)} free on disk</div>
      {:else}
        <div class="skel line" style="width: 60%; margin-top: 10px"></div>
        <div class="skel line" style="width: 100%; margin-top: 12px"></div>
      {/if}
    </div>
    <div class="card stat">
      <div class="stat-head"><RefreshCw size={15} />Playing now</div>
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
        <div class="faint small">open sessions</div>
      {/if}
    </div>
    <div class="card stat">
      <div class="stat-head"><Users size={15} />Users</div>
      <div class="stat-n">{stats?.users ?? users?.length ?? "–"}</div>
      <div class="faint small">{users?.filter((u) => u.is_admin).length ?? 0} admin</div>
    </div>
  </div>

  <h2 class="section-title">Users</h2>
  {#if error}
    <div class="empty"><h3>Couldn't load users</h3><span>{error}</span></div>
  {:else}
    <div class="card table-card">
      <table class="list">
        <thead><tr><th>User</th><th>Role</th><th>Created</th><th>Enabled</th><th></th></tr></thead>
        <tbody>
          {#if !users}
            {#each Array(3) as _, i (i)}<tr><td colspan="5"><div class="skel line" style="width: {70 - i * 12}%"></div></td></tr>{/each}
          {:else}
            {#each users as u, i (u.id)}
              {@const me = u.id === session.user?.id}
              <tr class="stagger" style="--i: {i}" class:off={u.disabled}>
                <td>
                  <span class="who"><span class="avatar">{u.username.slice(0, 1).toUpperCase()}</span>{u.username}{#if me}<span class="chip">you</span>{/if}</span>
                </td>
                <td>
                  <button class="chip role" class:accent={u.is_admin} disabled={me} title={me ? "" : "Click to change"} onclick={() => patch(u, { is_admin: !u.is_admin }, u.is_admin ? "No longer admin" : "Now admin")}>
                    {u.is_admin ? "Admin" : "User"}
                  </button>
                </td>
                <td class="faint">{relTime(u.created_at)}</td>
                <td><Switch checked={!u.disabled} disabled={me} onchange={(v) => patch(u, { disabled: !v }, v ? "Enabled" : "Disabled. Their sessions were closed.")} /></td>
                <td class="acts">
                  <button class="icon-btn" title="Reset password" onclick={() => resetPw(u)}><KeyRound size={15} /></button>
                  <button class="icon-btn del" title="Delete user" disabled={me} onclick={() => remove(u)}><Trash2 size={15} /></button>
                </td>
              </tr>
            {/each}
          {/if}
        </tbody>
      </table>
    </div>
  {/if}

  <h2 class="section-title">Torrents <span class="live"><span class="dot"></span>live</span></h2>
  <div class="card table-card">
    <table class="list">
      <thead><tr><th>Name</th><th>Progress</th><th>Down</th><th>Up</th><th>Peers</th><th>Last used</th><th></th></tr></thead>
      <tbody>
        {#if !torrents}
          {#each Array(2) as _, i (i)}<tr><td colspan="7"><div class="skel line" style="width: {80 - i * 20}%"></div></td></tr>{/each}
        {:else if !torrents.length}
          <tr><td colspan="7" class="faint none">No torrents in the cache.</td></tr>
        {:else}
          {#each torrents as t (t.info_hash)}
            {@const pct = t.size_bytes ? (t.downloaded_bytes / t.size_bytes) * 100 : 0}
            <tr>
              <td class="tname"><span class="ellipsis" title={t.name}>{t.name}</span><span class="faint small">{bytes(t.size_bytes)} · {t.state}</span></td>
              <td class="tprog">
                <div class="progress"><div class="progress-fill" style="width: {pct}%"></div></div>
                <span class="faint small">{pct.toFixed(0)}%</span>
              </td>
              <td class="num" class:active={t.down_bps > 0}>{speed(t.down_bps)}</td>
              <td class="num faint">{speed(t.up_bps)}</td>
              <td class="num">{t.peers}</td>
              <td class="faint">{relTime(t.last_used_at)}</td>
              <td class="acts"><button class="icon-btn del" title="Remove torrent and files" onclick={() => dropTorrent(t)}><Trash2 size={15} /></button></td>
            </tr>
          {/each}
        {/if}
      </tbody>
    </table>
  </div>
</div>

{#if creating}
  <Drawer title="New user" onclose={() => (creating = false)} onsave={create}>
    <label class="field">
      Username
      <!-- svelte-ignore a11y_autofocus -->
      <input bind:value={nu.username} autofocus autocomplete="off" spellcheck="false" />
    </label>
    <label class="field">
      Password
      <input type="password" bind:value={nu.password} autocomplete="new-password" />
      <span class="hint">They can change it later in Settings.</span>
    </label>
    <Switch bind:checked={nu.is_admin}>Admin (can manage users and the cache)</Switch>
    {#snippet footer()}
      <button class="btn" onclick={() => (creating = false)}>Cancel</button>
      <button class="btn primary" disabled={saving || !nu.username.trim() || !nu.password} onclick={create}><Plus size={15} />Create user</button>
    {/snippet}
  </Drawer>
{/if}

<style>
  .page {
    padding: 26px var(--gutter) 40px;
    max-width: 1600px;
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
  .avatar {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border-radius: 50%;
    font-weight: 700;
    font-size: 12px;
    color: var(--accent-ink);
    background: var(--primary-bg);
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
  @media (max-width: 700px) {
    .stats-grid {
      grid-template-columns: 1fr 1fr;
    }
    .stats-grid > :first-child {
      grid-column: 1 / -1;
    }
  }
</style>
