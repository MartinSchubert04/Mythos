<script lang="ts">
  // Settings → Connection (desktop y APK): Auto / Solo server / Solo local, el aviso al cambiar,
  // y el estado del server del dispositivo (cambios sin subir, ultima sincronizacion, "Sync now").
  // La logica esta en lib/local.svelte.ts; ver docs/local-mode.md.
  import { Cloud, HardDrive, LoaderCircle, RefreshCw } from "@lucide/svelte";
  import { ApiError, errText } from "../lib/api";
  import { serverText } from "../lib/i18n/server";
  import { relTime } from "../lib/format";
  import { conn, connDetail, loginLocal, refreshStatus, setMode, syncNow, type ConnMode } from "../lib/local.svelte";
  import { isAndroid } from "../lib/platform";
  import { prefs, savePrefs } from "../lib/prefs.svelte";
  import { logout, session } from "../lib/session.svelte";
  import { toast } from "../lib/toast.svelte";
  import { t, type Key } from "../lib/i18n.svelte";
  import Section from "./Section.svelte";
  import Seg from "./Seg.svelte";
  import Switch from "./Switch.svelte";

  const MODES: { value: ConnMode; label: string }[] = [
    { value: "auto", get label() { return t("account.conn.auto"); } },
    { value: "server", get label() { return t("account.conn.serverOnly"); } },
    { value: "local", get label() { return t("account.conn.localOnly"); } },
  ];
  const HINTS: Record<ConnMode, Key> = {
    auto: "account.conn.hintAuto",
    server: "account.conn.hintServer",
    local: "account.conn.hintLocal",
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
      toast(t("account.conn.enrolled"));
      refreshStatus();
    } catch (err) {
      toast(err instanceof ApiError && err.status === 401 ? t("account.wrongPassword") : errText(err), true);
    } finally {
      enrolling = false;
    }
  }

  async function sync() {
    syncing = true;
    try {
      const r = await syncNow();
      toast(r.pushed || r.pulled ? t("account.conn.synced", { up: r.pushed, down: r.pulled }) : t("account.conn.inSync"));
    } catch (e) {
      toast(e instanceof ApiError && e.status === 401 ? t("account.conn.signInToSync") : errText(e), true);
    } finally {
      syncing = false;
    }
  }

  const LIMITS: Record<"tv" | "low_ram", Key> = {
    tv: "account.conn.limitTv",
    low_ram: "account.conn.limitRam",
  };

  const summary = $derived(`${MODES.find((m) => m.value === conn.mode)?.label} · ${conn.on === "local" ? t("account.conn.onDevice") : t("account.conn.onServer")}`);
</script>

<Section id="connection" title={t("account.conn.title")} icon={conn.on === "local" ? HardDrive : Cloud} {summary}>
  {#if conn.unavailable}
    <!-- TV box o poca RAM: el server embebido no corre (apps/local-server, docs/local-mode.md). -->
    <div class="field">
      {t("account.conn.use")}
      <span class="now"><Cloud size={14} />{t("account.conn.serverOnly")}</span>
      <span class="hint">{t(LIMITS[conn.unavailable])}</span>
    </div>
  {:else}
    <div class="field">
      {t("account.conn.use")}
      <Seg value={mode} options={MODES} onchange={(v) => ((mode = v), pick(v))} />
      <span class="hint">{t(HINTS[mode])}</span>
    </div>

    {#if needsPassword}
      <form class="enroll" onsubmit={enroll}>
        <span class="what">{t("account.conn.enrollHint")}</span>
        <div class="row">
          <input type="password" placeholder={t("account.password")} bind:value={password} autocomplete="current-password" />
          <button class="btn primary" disabled={!password || enrolling || busy}>
            {#if enrolling}<LoaderCircle size={14} class="spin" />{/if}{t("account.conn.enable")}
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
            <RefreshCw size={13} class={syncing ? "spin" : ""} />{t("account.conn.syncNow")}
          </button>
        </div>
        {#if s}
          <span class="faint small">
            {s.last_sync_at ? t("account.conn.lastSync", { when: relTime(new Date(s.last_sync_at).toISOString()) }) : t("account.conn.notSynced")}{s.pending_changes
              ? ` · ${t("account.conn.waiting", { n: s.pending_changes })}`
              : ""}{s.last_sync_error && s.remote_reachable ? ` · ${serverText(s.last_sync_error, s.last_sync_error_code, { e: s.last_sync_error })}` : ""}
          </span>
        {/if}
        {#if conn.needsSignIn}
          <div class="row">
            <span class="grow faint small">{t("account.conn.sessionEnded")}</span>
            <button class="btn sm" onclick={() => logout()}>{t("account.conn.signInAgain")}</button>
          </div>
        {/if}
        {#if conn.error}<span class="err small">{t("account.conn.localFailed", { e: conn.error })}</span>{/if}
        {#if !conn.transcode}
          <span class="faint small">
            {t("account.conn.directOnly")}{isAndroid
              ? ""
              : ` ${t("account.conn.installFfmpeg")}`}
          </span>
        {/if}
      </div>
    {/if}

    {#if conn.mode === "auto"}
      <Switch bind:checked={prefs.localNotice} onchange={() => savePrefs(false)}>{t("account.conn.notice")}</Switch>
    {/if}
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
