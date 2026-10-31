<script lang="ts">
  // Dispositivos con la sesion abierta (Settings → Account): cada login es uno. Se puede cerrar
  // uno o todos menos este. Nunca se ve el token, solo lo que el server guardo del login.
  // Con `user` (Admin → Users): las sesiones de esa cuenta, para cerrar una (p. ej. una TV, que no
  // vence sola).
  import { Laptop, LogOut, Monitor, Smartphone, Terminal, Tv, X } from "@lucide/svelte";
  import { api, errText } from "../lib/api";
  import { ask } from "../lib/confirm.svelte";
  import { flipMs } from "../lib/motion";
  import { attempt, toast } from "../lib/toast.svelte";
  import { agoMs, fullDate, prettyUa } from "../lib/traffic";
  import type { SessionInfo, User } from "../lib/types";
  import { flip } from "svelte/animate";

  let { user }: { user?: User } = $props();

  let list = $state<SessionInfo[] | null>(null);
  let error = $state("");

  function load() {
    (user ? api.userSessions(user.id) : api.sessions())
      .then((l) => {
        // La de este dispositivo primero, despues la mas usada.
        list = [...l].sort((a, b) => Number(b.current) - Number(a.current) || b.last_used_at - a.last_used_at);
        error = "";
      })
      .catch((e) => (error = errText(e)));
  }
  load();

  const others = $derived(list?.filter((s) => !s.current).length ?? 0);
  // Con muchas sesiones (logins viejos que nunca se cerraron) se ven las 5 mas recientes.
  const FEW = 5;
  let all = $state(false);
  const shown = $derived(all ? (list ?? []) : (list ?? []).slice(0, FEW));
  const icons = { desktop: Monitor, phone: Smartphone, tv: Tv, tool: Terminal };

  function title(s: SessionInfo) {
    const ua = prettyUa(s.user_agent);
    return { name: s.device?.trim() || ua.name, sub: s.device?.trim() ? ua.name : "", Icon: s.device && /laptop/i.test(s.device) ? Laptop : icons[ua.kind] };
  }

  async function revoke(s: SessionInfo) {
    const t = title(s);
    const ok = await ask({ title: `Sign out ${t.name}?`, body: "That device will need the password to come back.", confirm: "Sign out device", danger: true });
    const done = () => (user ? api.revokeUserSession(user.id, s.id) : api.revokeSession(s.id));
    if (ok && (await attempt(done, "Device signed out"))) list = list?.filter((x) => x.id !== s.id) ?? null;
  }

  async function revokeOthers() {
    const ok = await ask({
      title: "Sign out all other devices?",
      body: `${others} other device${others === 1 ? "" : "s"} will need the password to come back. This one stays signed in.`,
      confirm: "Sign out others",
      danger: true,
    });
    if (!ok) return;
    try {
      const { revoked } = await api.revokeOtherSessions();
      list = list?.filter((x) => x.current) ?? null;
      toast(revoked === 1 ? "1 device signed out" : `${revoked} devices signed out`);
    } catch (e) {
      toast(errText(e), true);
      load();
    }
  }
</script>

<div class="devices">
  {#if error && !list}
    <div class="msg">Couldn't load {user ? "the" : "your"} devices: {error} <button class="btn sm" onclick={load}>Retry</button></div>
  {:else if !list}
    {#each Array(2) as _, i (i)}<div class="dev"><span class="skel ico"></span><div class="skel line" style="width: {55 - i * 12}%"></div></div>{/each}
  {:else}
    {#each shown as s (s.id)}
      {@const t = title(s)}
      <div class="dev" animate:flip={{ duration: flipMs() }}>
        <span class="ico" class:cur={s.current}><t.Icon size={16} /></span>
        <div class="info">
          <div class="name">
            <span class="ellipsis">{t.name}</span>
            {#if s.current}<span class="chip accent">This device</span>{/if}
            {#if s.persistent}<span class="chip" title="TV sessions stay signed in until someone signs them out here">Never expires</span>{/if}
          </div>
          <div class="sub" title={s.user_agent ?? ""}>
            {#if t.sub}<span>{t.sub}</span>{/if}
            {#if s.last_ip}<span class="mono">{s.last_ip}</span>{/if}
            <span title="Last used {fullDate(s.last_used_at * 1000)} · signed in {fullDate(s.created_at * 1000)}">
              {s.current ? "Active now" : `Last used ${agoMs(s.last_used_at * 1000)}`}
            </span>
          </div>
        </div>
        {#if !s.current}
          <button class="icon-btn del" title="Sign out this device" onclick={() => revoke(s)}><X size={15} /></button>
        {/if}
      </div>
    {/each}
    {#if list.length > FEW}
      <button class="btn ghost sm more" onclick={() => (all = !all)}>{all ? "Show fewer" : `Show ${list.length - FEW} more`}</button>
    {/if}
    {#if user}
      {#if !list.length}<div class="msg">{user.username} isn't signed in anywhere.</div>{/if}
    {:else if others}
      <div><button class="btn" onclick={revokeOthers}><LogOut size={14} />Sign out all other devices</button></div>
    {:else}
      <div class="msg">Only this device is signed in.</div>
    {/if}
  {/if}
</div>

<style>
  .devices {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .dev {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: center;
    gap: 12px;
    padding: 9px 10px;
    border: var(--border) solid var(--line);
    border-radius: var(--radius-sm);
    background: var(--panel-2);
  }
  .ico {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: var(--radius-sm);
    background: var(--panel);
    color: var(--text-2);
  }
  .ico.cur {
    color: var(--accent);
  }
  .skel.ico {
    background: var(--panel);
  }
  .info {
    min-width: 0;
  }
  .name {
    display: flex;
    align-items: center;
    gap: 8px;
    font-weight: 600;
    min-width: 0;
  }
  .sub {
    display: flex;
    flex-wrap: wrap;
    gap: 2px 10px;
    font-size: 12px;
    color: var(--text-3);
    margin-top: 2px;
  }
  .sub .mono {
    font-size: 11.5px;
  }
  .del:hover {
    color: var(--danger);
  }
  .more {
    align-self: flex-start;
  }
  .msg {
    font-size: 12.5px;
    color: var(--text-3);
  }
</style>
