<script lang="ts">
  // Settings → Connection (desktop y APK): Auto / Solo server / Solo local, el aviso al cambiar,
  // y el estado del server del dispositivo (cambios sin subir, ultima sincronizacion, "Sync now").
  // La logica esta en lib/local.svelte.ts; ver docs/local-mode.md.
  import { Cloud, HardDrive, LoaderCircle, RefreshCw } from "@lucide/svelte";
  import { ApiError, errText } from "../lib/api";
  import { relTime } from "../lib/format";
  import { conn, connDetail, loginLocal, refreshStatus, setMode, syncNow, type ConnMode } from "../lib/local.svelte";
  import { isAndroid } from "../lib/platform";
  import { prefs, savePrefs } from "../lib/prefs.svelte";
  import { logout, session } from "../lib/session.svelte";
  import { toast } from "../lib/toast.svelte";
  import Section from "./Section.svelte";
  import Seg from "./Seg.svelte";
  import Switch from "./Switch.svelte";

  const MODES: { value: ConnMode; label: string }[] = [
    { value: "auto", label: "Auto" },
    { value: "server", label: "Server only" },
    { value: "local", label: "Local only" },
  ];
  const HINTS: Record<ConnMode, string> = {
    auto: "Uses your server while it responds. If it goes down, Mythos keeps working on this device (catalog, streams, progress) and syncs everything when the server is back.",
    server: "Always uses your server, nothing runs on this device. If the server is down, Mythos waits for it.",
    local: "Always plays and stores everything on this device, even when your server is up. Progress, library and settings still sync in the background whenever the server is reachable.",
  };

  let mode = $state<ConnMode>(conn.mode);
  let busy = $state(false);
  let password = $state("");
  let enrolling = $state(false);
  let syncing = $state(false);

  refreshStatus();

  /** La cuenta no esta en el dispositivo todavia (hace falta la contraseña una vez). */
  const needsPassword = $derived(conn.mode !== "server" && !conn.enrolled);
  const s = $derived(conn.status);

  async function pick(m: ConnMode) {
    busy = true;
    try {
      const r = await setMode(m);
      // Solo server sin sesion del server (se habia entrado sin conexion): al login.
      if (r === "signin" && m === "server") await logout();
      refreshStatus();
    } catch (e) {
      toast(errText(e), true);
      mode = conn.mode;
    } finally {
      busy = false;
    }
  }

  async function enroll(e: Event) {
    e.preventDefault();
    if (!session.user || !password) return;
    enrolling = true;
    try {
      await loginLocal(session.user.username, password);
      password = "";
      toast("This device can now keep working without the server");
      refreshStatus();
    } catch (err) {
      toast(err instanceof ApiError && err.status === 401 ? "Wrong password." : errText(err), true);
    } finally {
      enrolling = false;
    }
  }

  async function sync() {
    syncing = true;
    try {
      const r = await syncNow();
      toast(r.pushed || r.pulled ? `Synced · ${r.pushed} up, ${r.pulled} down` : "Already in sync");
    } catch (e) {
      toast(e instanceof ApiError && e.status === 401 ? "Sign in again to sync with your server." : errText(e), true);
    } finally {
      syncing = false;
    }
  }

  const summary = $derived(`${MODES.find((m) => m.value === conn.mode)?.label} · ${conn.on === "local" ? "on this device" : "on the server"}`);
</script>

<Section id="connection" title="Connection" icon={conn.on === "local" ? HardDrive : Cloud} {summary}>
  <div class="field">
    Use
    <Seg value={mode} options={MODES} onchange={(v) => ((mode = v), pick(v))} />
    <span class="hint">{HINTS[mode]}</span>
  </div>

  {#if needsPassword}
    <form class="enroll" onsubmit={enroll}>
      <span class="what">Enter your password once so this device can keep working without the server.</span>
      <div class="row">
        <input type="password" placeholder="Password" bind:value={password} autocomplete="current-password" />
        <button class="btn primary" disabled={!password || enrolling || busy}>
          {#if enrolling}<LoaderCircle size={14} class="spin" />{/if}Enable
        </button>
      </div>
    </form>
  {:else if conn.mode !== "server"}
    <div class="state">
      <div class="row">
        <span class="grow now">
          {#if conn.on === "local"}<HardDrive size={14} />{:else}<Cloud size={14} />{/if}
          {connDetail()}
        </span>
        <button class="btn sm" onclick={sync} disabled={syncing || !conn.enrolled}>
          <RefreshCw size={13} class={syncing ? "spin" : ""} />Sync now
        </button>
      </div>
      {#if s}
        <span class="faint small">
          {s.last_sync_at ? `Last sync ${relTime(new Date(s.last_sync_at).toISOString())}` : "Not synced yet"}{s.pending_changes
            ? ` · ${s.pending_changes} waiting`
            : ""}{s.last_sync_error && s.remote_reachable ? ` · ${s.last_sync_error}` : ""}
        </span>
      {/if}
      {#if conn.needsSignIn}
        <div class="row">
          <span class="grow faint small">Your server session ended. Sign in again to sync and use the server.</span>
          <button class="btn sm" onclick={() => logout()}>Sign in again</button>
        </div>
      {/if}
      {#if conn.error}<span class="err small">The local server didn't start: {conn.error}</span>{/if}
      {#if !conn.transcode}
        <span class="faint small">
          On this device only files it can play directly work (Mythos picks those first).{isAndroid
            ? ""
            : " Install ffmpeg (on the PATH) to convert the rest."}
        </span>
      {/if}
    </div>
  {/if}

  {#if conn.mode === "auto"}
    <Switch bind:checked={prefs.localNotice} onchange={() => savePrefs(false)}>Show a notice when switching to local</Switch>
  {/if}
</Section>

<style>
  .enroll,
  .state {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .enroll input {
    flex: 1;
    max-width: 320px;
  }
  .what {
    color: var(--text-2);
    font-size: 13px;
  }
  .now {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-2);
    font-size: 13px;
    min-width: 0;
  }
  .now :global(svg) {
    flex: none;
    color: var(--text-3);
  }
  .grow {
    flex: 1;
  }
  .small {
    font-size: 12px;
    line-height: 1.5;
  }
  .err {
    color: var(--danger);
  }
</style>
