<script lang="ts">
  // Ajustes: tema y paleta con muestras en vivo (como Ethereon), reproduccion, subtitulos,
  // Real-Debrid y la cuenta. Lo de la interfaz se guarda solo (local y en el server).
  import { Captions, Download, Keyboard, KeyRound, LogOut, Monitor, Palette, Play, RefreshCw, Server, Smartphone, UserRound } from "@lucide/svelte";
  import { MonitorSmartphone, Users } from "@lucide/svelte";
  import { conn } from "../lib/local.svelte";
  import AvatarPicker from "../components/AvatarPicker.svelte";
  import ConnectionSettings from "../components/ConnectionSettings.svelte";
  import Devices from "../components/Devices.svelte";
  import Section from "../components/Section.svelte";
  import Seg from "../components/Seg.svelte";
  import Select from "../components/Select.svelte";
  import Switch from "../components/Switch.svelte";
  import { api, ApiError, errText } from "../lib/api";
  import { ask } from "../lib/confirm.svelte";
  import { isAndroid, isDesktop, isRemoteShell, pickServer, remoteBase } from "../lib/platform";
  import { isTizen } from "../lib/tizen";
  import ServerAddresses from "../components/ServerAddresses.svelte";
  import { LANGS, prefs, saveLook, savePrefs, setTheme, setUiLang } from "../lib/prefs.svelte";
  import { i18n, t, UI_LANGS } from "../lib/i18n.svelte";
  import { ALL_KEYS as KEYS } from "../lib/shortcuts";
  import { AUDIO_LANGS } from "../lib/streamlang";
  import { keepScroll } from "../lib/scroll";
  import { profiles, syncProfile } from "../lib/profiles.svelte";
  import { logout, session } from "../lib/session.svelte";
  import { setTvPref, tv } from "../lib/tv.svelte";
  import { MIN_OPACITY, glass, setGlass } from "../lib/glass.svelte";
  import { checkUpdate, installUpdate, leaveServer, remoteUi, setRemoteUi, shell } from "../lib/shell.svelte";
  import { THEMES, currentPalette, look, palettesOf, type ThemeId } from "../lib/themes.svelte";
  import { attempt, toast } from "../lib/toast.svelte";
  import type { PreferredQuality } from "../lib/types";

  // El tema es de este usuario (lib/themes.svelte.ts): va en sus preferencias del server. Se lee
  // de `look` (reactivo), asi sigue al usuario si cambia con la pantalla abierta.
  const theme = $derived(look.theme);
  const palette = $derived(currentPalette(look.theme));
  // La muestra de cada tema usa la paleta que tiene elegida.
  const swatchOf = (th: ThemeId) => palettesOf(th).find((p) => p.id === currentPalette(th))?.swatch ?? ["#000", "#000", "#000"];
  const themeName = $derived(THEMES.find((th) => th.id === theme)?.name ?? "");
  const paletteName = $derived(palettesOf(theme).find((p) => p.id === palette)?.name ?? "");

  function pickTheme(th: ThemeId) {
    setTheme(th);
  }
  function pickPalette(p: string) {
    setTheme(theme, p);
  }

  // ---- reproduccion (van al server como campos propios de Settings)
  let quality = $state<PreferredQuality>(session.settings?.preferred_quality ?? "1080p");
  let maxGb = $state<number>(session.settings?.max_size_gb ?? 0);
  async function saveServer(patch: { preferred_quality?: PreferredQuality; max_size_gb?: number | null }) {
    await attempt(async () => {
      const s = await api.saveSettings(patch);
      session.settings = s;
    });
  }

  const QUALITIES: { value: PreferredQuality; label: string }[] = [
    { value: "2160p", label: "4K" },
    { value: "1080p", label: "1080p" },
    { value: "720p", label: "720p" },
  ];

  // Los atajos viven en lib/shortcuts.ts (el "?" del reproductor muestra los mismos). En el celular
  // (pantalla tactil) y en la TV (el control: las flechas del reproductor hacen otra cosa) no hay
  // teclado: la seccion no se muestra.
  const noKeyboard = tv.on || (typeof matchMedia !== "undefined" && matchMedia("(pointer: coarse)").matches);

  const SIZES = [
    { value: 80, get label() { return t("account.subs.small"); } },
    { value: 100, get label() { return t("account.subs.normal"); } },
    { value: 125, get label() { return t("account.subs.large"); } },
    { value: 150, get label() { return t("account.subs.huge"); } },
  ];

  // ---- Real-Debrid
  let rdKey = $state("");
  const hasRd = $derived(!!session.settings?.has_rd_key);
  async function saveRd(value: string) {
    const ok = await attempt(async () => {
      session.settings = await api.saveSettings({ rd_key: value });
    }, value ? t("account.rd.saved") : t("account.rd.removed"));
    if (ok) rdKey = "";
  }

  // ---- cuenta
  let oldPw = $state("");
  let newPw = $state("");
  async function changePw() {
    if (newPw.length < 4) return attempt(() => Promise.reject(new Error(t("account.pw.short"))));
    if (await attempt(() => api.changePassword(oldPw, newPw), t("account.pw.changed"))) oldPw = newPw = "";
  }
  let newName = $state(session.user?.username ?? "");
  let renaming = $state(false);
  async function rename() {
    const name = newName.trim();
    if (!name || name === session.user?.username) return;
    renaming = true;
    try {
      const u = await api.changeUsername(name);
      session.user = u;
      newName = u.username;
      syncProfile(u);
      toast(t("account.renamed", { name: u.username }));
    } catch (e) {
      toast(e instanceof ApiError && e.status === 409 ? t("account.nameTaken") : errText(e), true);
    } finally {
      renaming = false;
    }
  }

  async function signOut() {
    const body = tv.on ? t("account.out.tvBody") : t("account.out.body");
    if (await ask({ title: tv.on ? t("account.out.tvTitle") : t("account.out.title"), body, confirm: tv.on ? t("account.removeProfile") : t("account.signOut") })) logout();
  }
  /** Lo mismo que salir (las dos sesiones, en desktop y Android): queda el login con el server. En
   *  la TV, los perfiles guardados (sin salir de nada). */
  async function switchAccount() {
    if (tv.on) {
      profiles.picking = true;
      return;
    }
    if (await ask({ title: t("account.switch.title"), body: t("account.switch.body"), confirm: t("account.switch.confirm") })) logout();
  }

  // ---- Android y desktop: actualizaciones desde los Releases de GitHub
  let checking = $state(false);
  let upToDate = $state(false);
  async function lookForUpdate() {
    checking = true;
    upToDate = false;
    try {
      upToDate = !(await checkUpdate()) && !shell.update;
    } catch (e) {
      toast(String(e), true);
    } finally {
      checking = false;
    }
  }
  let useServerUi = $state(remoteUi.on);
  async function toggleServerUi(on: boolean) {
    if (!(await setRemoteUi(on, remoteBase()))) {
      useServerUi = false;
      toast(t("account.app.noServerUi"), true);
    }
  }
  async function updateNow() {
    try {
      if (!(await installUpdate()))
        toast(isAndroid ? t("account.app.allowInstall") : t("account.app.gone"), !isAndroid);
    } catch (e) {
      toast(String(e), true);
    }
  }
</script>

<div class="page" use:keepScroll={"settings"}>
  <div class="page-head"><h1>{t("nav.settings")}</h1></div>

  <div class="sections">
    <Section id="appearance" title={t("account.look.title")} icon={Palette} summary="{themeName} · {paletteName}">
      <div class="field ui-lang">
        {t("common.uiLanguage")}
        <Seg value={i18n.lang} options={UI_LANGS} onchange={(v) => setUiLang(v)} />
        <span class="hint">{t("common.uiLanguageHint")}</span>
      </div>
      <div class="sub">{t("account.look.theme")}</div>
      <div class="themes">
        {#each THEMES as th, i (th.id)}
          {@const sw = theme === th.id ? (palettesOf(th.id).find((p) => p.id === palette)?.swatch ?? swatchOf(th.id)) : swatchOf(th.id)}
          <button class="theme card stagger" style="--i: {i}" class:on={theme === th.id} onclick={() => pickTheme(th.id)}>
            <div class="sw" style="background: linear-gradient(135deg, {sw[0]} 0 45%, {sw[1]} 45% 75%, {sw[2]} 75%)">
              <span class="mini-poster" style="background: {sw[1]}; border-color: {sw[2]}"></span>
              <span class="mini-poster" style="background: {sw[0]}; border-color: {sw[2]}"></span>
              <span class="mini-bar" style="background: {sw[2]}"></span>
            </div>
            <span>{th.name}</span>
          </button>
        {/each}
      </div>
      <div class="sub">{t("account.look.paletteFor", { theme: themeName })}</div>
      <div class="palettes">
        {#each palettesOf(theme) as p (p.id)}
          <button class="pal" class:on={palette === p.id} onclick={() => pickPalette(p.id)}>
            <span class="dots">
              {#each p.swatch as c (c)}<span style="background: {c}"></span>{/each}
            </span>
            {p.name}
          </button>
        {/each}
      </div>
      <Switch bind:checked={prefs.reduceMotion} onchange={() => savePrefs()}>{t("account.look.reduceMotion")}</Switch>
      {#if !isAndroid && !isTizen}
        <!-- Modo TV en la web (lib/tv.svelte.ts): en la app de Android y en Samsung es automatico. -->
        <Switch checked={tv.on} onchange={(v) => setTvPref(v ? "on" : "off")}>{t("account.look.tv")}</Switch>
        <span class="app-hint">
          {t("account.look.tvHint")}
          {#if tv.pref}
            <button class="btn ghost sm" onclick={() => setTvPref(null)}>{t("account.look.tvDetect")}</button>
          {:else}
            {tv.auto ? t("account.look.tvAutoYes") : t("account.look.tvAutoNo")}
          {/if}
        </span>
      {/if}
      {#if isDesktop}
        <Switch checked={glass.on} onchange={(v) => (setGlass(v), saveLook("glass"))}>{t("account.look.glass")}</Switch>
        {#if glass.on}
          <label class="field glass-op">
            <span class="glass-name">{t("account.look.opacity")} <span class="faint">{Math.round(glass.opacity * 100)}%</span></span>
            <input
              type="range"
              min={MIN_OPACITY}
              max="1"
              step="0.01"
              value={glass.opacity}
              style="--v: {((glass.opacity - MIN_OPACITY) / (1 - MIN_OPACITY)) * 100}%"
              oninput={(e) => setGlass(true, +e.currentTarget.value, false)}
              onchange={(e) => (setGlass(true, +e.currentTarget.value), saveLook("glass"))}
            />
            <span class="hint">{t("account.look.opacityHint")}</span>
          </label>
        {/if}
      {/if}
    </Section>

    <Section
      id="playback"
      title={t("account.play.title")}
      icon={Play}
      summary="{quality}{maxGb ? ` · ${t('account.play.upTo', { n: maxGb })}` : ''} · {prefs.autoplayNext ? t('account.play.autoOn') : t('account.play.autoOff')}"
    >
      <div class="grid2">
        <div class="field">
          {t("account.play.quality")}
          <Seg
            value={quality}
            options={QUALITIES}
            onchange={(v) => ((quality = v), saveServer({ preferred_quality: v }))}
          />
          <span class="hint">{t("account.play.qualityHint")}</span>
        </div>
        <div class="field">
          {t("account.play.maxSize")}
          <Select
            bind:value={maxGb}
            options={[
              { value: 0, label: t("account.play.noLimit") },
              ...[2, 4, 8, 15, 25, 40, 60].map((n) => ({ value: n, label: `${n} GB` })),
            ]}
            onchange={(v) => saveServer({ max_size_gb: v || null })}
          />
          <span class="hint">{t("account.play.maxSizeHint")}</span>
        </div>
        <div class="field">
          {t("account.play.audio")}
          <Select bind:value={prefs.audioLang} options={AUDIO_LANGS} onchange={() => savePrefs()} />
          <span class="hint">{t("account.play.audioHint")}</span>
        </div>
      </div>
      <Switch bind:checked={prefs.autoplayNext} onchange={() => savePrefs()}>{t("account.play.autoplay")}</Switch>
      <Switch bind:checked={prefs.hideSpoilers} onchange={() => savePrefs()}>{t("account.play.spoilers")}</Switch>
    </Section>

    <Section
      id="subtitles"
      title={t("account.subs.title")}
      icon={Captions}
      summary="{LANGS.find((l) => l.value === prefs.subLang)?.label ?? prefs.subLang} · {SIZES.find((s) => s.value === prefs.subSize)?.label ?? `${prefs.subSize}%`}"
    >
      <div class="grid2">
        <div class="field">
          {t("account.subs.lang")}
          <Select bind:value={prefs.subLang} options={LANGS} onchange={() => savePrefs()} />
          <span class="hint">{prefs.subAlways ? t("account.subs.langHintAlways") : t("account.subs.langHint")}</span>
        </div>
        <div class="field">
          {t("account.subs.size")}
          <Seg bind:value={prefs.subSize} options={SIZES} onchange={(v) => ((prefs.subSize = v), savePrefs())} />
        </div>
      </div>
      <Switch bind:checked={prefs.subAlways} onchange={() => savePrefs()} disabled={!prefs.subLang}>{t("account.subs.always")}</Switch>
      <div class="sub-preview">
        <span style="font-size: calc(clamp(18px, 2.4vw, 40px) * {prefs.subSize / 100} * 0.7)">{t("account.subs.preview")}</span>
      </div>
    </Section>

    <Section id="rd" title="Real-Debrid" icon={KeyRound} summary={hasRd ? t("account.rd.stored") : t("account.rd.notSet")}>
      <div class="field">
        {t("account.rd.key")}
        <div class="row">
          <input
            type="password"
            bind:value={rdKey}
            placeholder={hasRd ? t("account.rd.replace") : t("account.rd.paste")}
            autocomplete="off"
            spellcheck="false"
          />
          <button class="btn primary" disabled={!rdKey.trim()} onclick={() => saveRd(rdKey.trim())}>{t("common.save")}</button>
          {#if hasRd}<button class="btn danger" onclick={() => saveRd("")}>{t("common.remove")}</button>{/if}
        </div>
        <span class="hint">{t("account.rd.hint")}</span>
      </div>
    </Section>

    {#if !noKeyboard}
      <Section id="keys" title={t("account.keys.title")} icon={Keyboard} summary={t("account.keys.summary")}>
        <div class="keys">
          {#each KEYS as [k, what] (k + what)}
            <div class="key-row"><span class="kbds">{#each k.split("  ·  ") as one, i (one)}{#if i}<span class="or">{t("player.or")}</span>{/if}<kbd>{one}</kbd>{/each}</span><span class="what">{what}</span></div>
          {/each}
        </div>
      </Section>
    {/if}

    {#if conn.available}<ConnectionSettings />{/if}

    <Section id="account" title={t("account.title")} icon={UserRound} summary={t("account.signedInAs", { name: session.user?.username ?? "" })}>
      <div class="sub">{t("account.avatar")}</div>
      <AvatarPicker />
      <div class="sub">{t("account.username")}</div>
      <div class="row">
        <input bind:value={newName} autocomplete="username" spellcheck="false" autocapitalize="off" maxlength="32" />
        <button class="btn" disabled={renaming || !newName.trim() || newName.trim() === session.user?.username} onclick={rename}>{t("account.rename")}</button>
      </div>
      {#if conn.on === "local"}<span class="app-hint">{t("account.renameLocal")}</span>{/if}
      <div class="sub">{t("account.pw.title")}</div>
      <div class="grid2">
        <input type="password" placeholder={t("account.pw.current")} bind:value={oldPw} autocomplete="current-password" />
        <input type="password" placeholder={t("account.pw.new")} bind:value={newPw} autocomplete="new-password" />
      </div>
      <div><button class="btn" disabled={!oldPw || !newPw} onclick={changePw}>{t("account.pw.title")}</button></div>
      {#if isRemoteShell}
        <div class="sub"><Server size={12} /> {t("account.server")}</div>
        <div class="row">
          <input value={location.host} readonly />
          <button class="btn" onclick={() => leaveServer()}>{t("account.changeServer")}</button>
        </div>
      {:else if pickServer}
        <div class="sub"><Server size={12} /> {t("account.server")}</div>
        <ServerAddresses />
      {/if}
      <div class="sub"><MonitorSmartphone size={12} /> {t("account.devices")}</div>
      <Devices />
      <div class="sep"></div>
      <div class="row">
        <button class="btn danger" onclick={signOut}><LogOut size={14} />{tv.on ? t("account.removeProfile") : t("account.signOut")}</button>
        <button class="btn ghost" onclick={switchAccount}><Users size={14} />{tv.on ? t("account.switchProfile") : t("account.switchAccount")}</button>
      </div>
    </Section>

    {#if isAndroid || isDesktop}
      <Section id="app" title={t("account.app.title")} icon={isAndroid ? Smartphone : Monitor} summary={shell.version ? t("account.app.version", { v: shell.version }) : isAndroid ? "Android" : "Desktop"}>
        <div class="row">
          <span class="grow">{t("account.app.name", { os: isAndroid ? "Android" : "Windows", v: shell.version ?? "" })}</span>
          {#if shell.update?.status === "ready"}
            <button class="btn primary" onclick={updateNow}><Download size={14} />{t("account.app.updateNow")}</button>
          {:else}
            <button class="btn" onclick={lookForUpdate} disabled={checking || !!shell.update}>
              <RefreshCw size={14} class={checking || shell.update ? "spin" : ""} />{t("account.app.check")}
            </button>
          {/if}
        </div>
        {#if shell.update}
          <span class="app-hint">
            {shell.update.status === "ready"
              ? isAndroid
                ? t("account.app.readyAndroid", { v: shell.update.version })
                : t("account.app.readyDesktop", { v: shell.update.version })
              : t("account.app.downloading", { v: shell.update.version, p: Math.round(shell.update.progress * 100) })}
          </span>
        {:else if upToDate}
          <span class="app-hint">{t("account.app.latest")}</span>
        {:else}
          <span class="app-hint">
            {isAndroid ? t("account.app.autoAndroid") : t("account.app.autoDesktop")}
          </span>
        {/if}
        {#if isAndroid}<Switch bind:checked={useServerUi} onchange={toggleServerUi}>{t("account.app.serverUi")}</Switch>{/if}
      </Section>
    {/if}
  </div>
</div>

<style>
  .page {
    padding: 26px var(--gutter) 40px;
  }
  .sections {
    display: flex;
    flex-direction: column;
    gap: 10px;
    max-width: 1040px;
  }
  .glass-op {
    max-width: 420px;
  }
  .glass-name {
    display: flex;
    justify-content: space-between;
  }
  .glass-op input {
    appearance: none;
    width: 100%;
    height: 18px;
    background: none;
    border: none;
    padding: 0;
    box-shadow: none;
    cursor: pointer;
  }
  .glass-op input::-webkit-slider-runnable-track {
    height: 4px;
    border-radius: 2px;
    background: linear-gradient(90deg, var(--accent) var(--v), var(--line-strong) var(--v));
  }
  .glass-op input::-webkit-slider-thumb {
    appearance: none;
    width: 14px;
    height: 14px;
    margin-top: -5px;
    border-radius: 50%;
    background: var(--accent);
    box-shadow: 0 1px 4px color-mix(in srgb, var(--accent) 40%, transparent);
  }
  .sub {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-3);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    margin-bottom: -6px;
  }
  .themes {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
    gap: 12px;
  }
  .theme {
    padding: 8px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    text-align: left;
    font-weight: 600;
    transition:
      transform 0.2s cubic-bezier(0.2, 0.8, 0.2, 1),
      outline-color 0.2s;
  }
  .theme:hover {
    transform: translateY(-2px);
  }
  .theme.on {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  .sw {
    position: relative;
    height: 64px;
    border-radius: var(--radius-sm);
    border: 1px solid color-mix(in srgb, var(--text) 18%, transparent);
    overflow: hidden;
  }
  /* Dos posters y una barra: una idea de como se ve el inicio con ese tema. */
  .mini-poster {
    position: absolute;
    bottom: 8px;
    width: 18px;
    height: 27px;
    border-radius: 3px;
    border: 1.5px solid;
  }
  .mini-poster:nth-child(1) {
    left: 10px;
  }
  .mini-poster:nth-child(2) {
    left: 33px;
  }
  .mini-bar {
    position: absolute;
    left: 10px;
    top: 9px;
    width: 40%;
    height: 5px;
    border-radius: 3px;
  }
  .palettes {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .pal {
    display: inline-flex;
    align-items: center;
    gap: 9px;
    height: 34px;
    padding: 0 13px 0 8px;
    border-radius: 999px;
    background: var(--panel);
    border: var(--border) solid var(--line);
    font-weight: 550;
    transition:
      border-color 0.15s,
      transform 0.15s;
  }
  .pal:hover {
    border-color: var(--line-strong);
    transform: translateY(-1px);
  }
  .pal.on {
    border-color: var(--accent);
    box-shadow: 0 0 0 1px var(--accent);
  }
  /* TV: lo elegido (tema y paleta) se marca por dentro; el anillo de afuera es solo del foco
     (styles/tv.css). Antes los dos eran el mismo anillo y no se sabia donde estaba parado. */
  :global(html[data-tv]) .theme.on {
    outline: none;
    border-color: var(--accent);
    box-shadow: inset 0 0 0 2px var(--accent);
  }
  :global(html[data-tv]) .pal.on {
    box-shadow: inset 0 0 0 1px var(--accent);
  }
  :global(html[data-tv]) .theme:focus-visible {
    outline: 3px solid var(--focus);
    outline-offset: 3px;
  }
  .dots {
    display: inline-flex;
  }
  .dots span {
    width: 16px;
    height: 16px;
    border-radius: 50%;
    border: 1.5px solid var(--panel);
    margin-left: -5px;
  }
  .dots span:first-child {
    margin-left: 0;
  }
  .sub-preview {
    position: relative;
    display: grid;
    place-items: end center;
    height: 90px;
    padding-bottom: 14px;
    border-radius: var(--radius-sm);
    background: linear-gradient(160deg, var(--panel-2), var(--media-bg));
    overflow: hidden;
  }
  .sub-preview span {
    font-family: var(--font-sub);
    font-weight: 600;
    color: var(--on-media);
    text-shadow: 0 2px 6px var(--media-bg);
    transition: font-size 0.2s;
  }
  .keys {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 380px), 1fr));
    gap: 2px 24px;
  }
  .key-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 6px 0;
    border-bottom: 1px solid color-mix(in srgb, var(--line) 60%, transparent);
  }
  .kbds {
    flex: none;
    min-width: 132px;
    display: flex;
    align-items: center;
    gap: 5px;
  }
  .or {
    font-size: 11px;
    color: var(--text-3);
  }
  .what {
    color: var(--text-2);
    font-size: 13px;
  }
  .sep {
    height: 1px;
    background: var(--line);
  }
  .row input {
    flex: 1;
  }
  .app-hint {
    font-size: 12.5px;
    color: var(--text-3);
    line-height: 1.5;
  }
  .row .grow {
    flex: 1;
    min-width: 0;
  }
</style>
