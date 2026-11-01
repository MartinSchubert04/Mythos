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
  import ServerAddresses from "../components/ServerAddresses.svelte";
  import { LANGS, prefs, savePrefs } from "../lib/prefs.svelte";
  import { ALL_KEYS as KEYS } from "../lib/shortcuts";
  import { AUDIO_LANGS } from "../lib/streamlang";
  import { keepScroll } from "../lib/scroll";
  import { profiles, syncProfile } from "../lib/profiles.svelte";
  import { logout, session } from "../lib/session.svelte";
  import { setTvPref, tv } from "../lib/tv.svelte";
  import { MIN_OPACITY, glass, setGlass } from "../lib/glass.svelte";
  import { checkUpdate, installUpdate, leaveServer, remoteUi, setRemoteUi, shell } from "../lib/shell.svelte";
  import { THEMES, applyTheme, currentPalette, currentTheme, palettesOf, type ThemeId } from "../lib/themes";
  import { attempt, toast } from "../lib/toast.svelte";
  import type { PreferredQuality } from "../lib/types";

  let theme = $state<ThemeId>(currentTheme());
  let palette = $state(currentPalette(currentTheme()));
  // La muestra de cada tema usa la paleta que tiene elegida.
  const swatchOf = (t: ThemeId) => palettesOf(t).find((p) => p.id === currentPalette(t))?.swatch ?? ["#000", "#000", "#000"];
  const themeName = $derived(THEMES.find((t) => t.id === theme)?.name ?? "");
  const paletteName = $derived(palettesOf(theme).find((p) => p.id === palette)?.name ?? "");

  function pickTheme(t: ThemeId) {
    theme = t;
    palette = currentPalette(t);
    applyTheme(t, palette);
    savePrefs();
  }
  function pickPalette(p: string) {
    palette = p;
    applyTheme(theme, p);
    savePrefs();
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
  // (pantalla tactil, sin modo TV) no hay teclado: la seccion no se muestra.
  const touchOnly = typeof matchMedia !== "undefined" && matchMedia("(pointer: coarse)").matches && !tv.on;

  const SIZES = [
    { value: 80, label: "Small" },
    { value: 100, label: "Normal" },
    { value: 125, label: "Large" },
    { value: 150, label: "Huge" },
  ];

  // ---- Real-Debrid
  let rdKey = $state("");
  const hasRd = $derived(!!session.settings?.has_rd_key);
  async function saveRd(value: string) {
    const ok = await attempt(async () => {
      session.settings = await api.saveSettings({ rd_key: value });
    }, value ? "Real-Debrid key saved" : "Real-Debrid key removed");
    if (ok) rdKey = "";
  }

  // ---- cuenta
  let oldPw = $state("");
  let newPw = $state("");
  async function changePw() {
    if (newPw.length < 4) return attempt(() => Promise.reject(new Error("Use at least 4 characters")));
    if (await attempt(() => api.changePassword(oldPw, newPw), "Password changed")) oldPw = newPw = "";
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
      toast(`You're now ${u.username}`);
    } catch (e) {
      toast(e instanceof ApiError && e.status === 409 ? "That username is taken." : errText(e), true);
    } finally {
      renaming = false;
    }
  }

  async function signOut() {
    const body = tv.on ? "This TV forgets this profile. You'll need the password to add it again." : "You'll need your password to come back.";
    if (await ask({ title: tv.on ? "Remove this profile?" : "Sign out?", body, confirm: tv.on ? "Remove profile" : "Sign out" })) logout();
  }
  /** Lo mismo que salir (las dos sesiones, en desktop y Android): queda el login con el server. En
   *  la TV, los perfiles guardados (sin salir de nada). */
  async function switchAccount() {
    if (tv.on) {
      profiles.picking = true;
      return;
    }
    if (await ask({ title: "Switch account?", body: "You'll sign out here and can sign in with another account on the same server.", confirm: "Switch" })) logout();
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
      toast("This server can't provide the interface. Update it and try again.", true);
    }
  }
  async function updateNow() {
    try {
      if (!(await installUpdate()))
        toast(isAndroid ? 'Allow "Install unknown apps" for Mythos, then tap Update now again.' : "The update is no longer available. Check again.", !isAndroid);
    } catch (e) {
      toast(String(e), true);
    }
  }
</script>

<div class="page" use:keepScroll={"settings"}>
  <div class="page-head"><h1>Settings</h1></div>

  <div class="sections">
    <Section id="appearance" title="Appearance" icon={Palette} summary="{themeName} · {paletteName}">
      <div class="sub">Theme</div>
      <div class="themes">
        {#each THEMES as t, i (t.id)}
          {@const sw = theme === t.id ? (palettesOf(t.id).find((p) => p.id === palette)?.swatch ?? swatchOf(t.id)) : swatchOf(t.id)}
          <button class="theme card stagger" style="--i: {i}" class:on={theme === t.id} onclick={() => pickTheme(t.id)}>
            <div class="sw" style="background: linear-gradient(135deg, {sw[0]} 0 45%, {sw[1]} 45% 75%, {sw[2]} 75%)">
              <span class="mini-poster" style="background: {sw[1]}; border-color: {sw[2]}"></span>
              <span class="mini-poster" style="background: {sw[0]}; border-color: {sw[2]}"></span>
              <span class="mini-bar" style="background: {sw[2]}"></span>
            </div>
            <span>{t.name}</span>
          </button>
        {/each}
      </div>
      <div class="sub">Palette for {themeName}</div>
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
      <Switch bind:checked={prefs.reduceMotion} onchange={() => savePrefs()}>Reduce motion (no animations or transitions)</Switch>
      {#if !isAndroid}
        <!-- Modo TV en la web (lib/tv.svelte.ts): en la app de Android es automatico. -->
        <Switch checked={tv.on} onchange={(v) => setTvPref(v ? "on" : "off")}>TV mode (remote control)</Switch>
        <span class="app-hint">
          Big focus ring and arrow-key navigation for a TV remote: arrows move, Enter selects, Back goes back. Saved in this browser.
          {#if tv.pref}
            <button class="btn ghost sm" onclick={() => setTvPref(null)}>Detect automatically</button>
          {:else}
            Detected automatically ({tv.auto ? "this looks like a TV" : "this doesn't look like a TV"}).
          {/if}
        </span>
      {/if}
      {#if isDesktop}
        <Switch checked={glass.on} onchange={(v) => setGlass(v)}>Frosted glass window (see your desktop blurred behind Mythos)</Switch>
        {#if glass.on}
          <label class="field glass-op">
            <span class="glass-name">Opacity <span class="faint">{Math.round(glass.opacity * 100)}%</span></span>
            <input
              type="range"
              min={MIN_OPACITY}
              max="1"
              step="0.01"
              value={glass.opacity}
              style="--v: {((glass.opacity - MIN_OPACITY) / (1 - MIN_OPACITY)) * 100}%"
              oninput={(e) => setGlass(true, +e.currentTarget.value, false)}
              onchange={(e) => setGlass(true, +e.currentTarget.value)}
            />
            <span class="hint">Lower is more see-through. Works with every theme.</span>
          </label>
        {/if}
      {/if}
    </Section>

    <Section
      id="playback"
      title="Playback"
      icon={Play}
      summary="{quality}{maxGb ? ` · up to ${maxGb} GB` : ''} · {prefs.autoplayNext ? 'autoplay on' : 'autoplay off'}"
    >
      <div class="grid2">
        <div class="field">
          Preferred quality
          <Seg
            value={quality}
            options={QUALITIES}
            onchange={(v) => ((quality = v), saveServer({ preferred_quality: v }))}
          />
          <span class="hint">Streams in this quality go first. Lower ones start faster on slow connections.</span>
        </div>
        <div class="field">
          Maximum file size
          <Select
            bind:value={maxGb}
            options={[
              { value: 0, label: "No limit" },
              ...[2, 4, 8, 15, 25, 40, 60].map((n) => ({ value: n, label: `${n} GB` })),
            ]}
            onchange={(v) => saveServer({ max_size_gb: v || null })}
          />
          <span class="hint">Bigger files are skipped when choosing the best stream.</span>
        </div>
        <div class="field">
          Audio language
          <Select bind:value={prefs.audioLang} options={AUDIO_LANGS} onchange={() => savePrefs()} />
          <span class="hint">Streams in this language go first (from tags like "Latino" or "Castellano"), and that audio track is picked when a file has several.</span>
        </div>
      </div>
      <Switch bind:checked={prefs.autoplayNext} onchange={() => savePrefs()}>Play the next episode automatically (10 second countdown)</Switch>
      <Switch bind:checked={prefs.hideSpoilers} onchange={() => savePrefs()}>Hide spoilers (blur images and descriptions of episodes you haven't started)</Switch>
    </Section>

    <Section
      id="subtitles"
      title="Subtitles"
      icon={Captions}
      summary="{LANGS.find((l) => l.value === prefs.subLang)?.label ?? prefs.subLang} · {SIZES.find((s) => s.value === prefs.subSize)?.label ?? `${prefs.subSize}%`}"
    >
      <div class="grid2">
        <div class="field">
          Preferred language
          <Select bind:value={prefs.subLang} options={LANGS} onchange={() => savePrefs()} />
          <span class="hint">Chosen by itself when a movie has it. Not shown when the audio is already in that language.</span>
        </div>
        <div class="field">
          Size
          <Seg bind:value={prefs.subSize} options={SIZES} onchange={(v) => ((prefs.subSize = v), savePrefs())} />
        </div>
      </div>
      <div class="sub-preview">
        <span style="font-size: calc(clamp(18px, 2.4vw, 40px) * {prefs.subSize / 100} * 0.7)">This is how subtitles will look.</span>
      </div>
    </Section>

    <Section id="rd" title="Real-Debrid" icon={KeyRound} summary={hasRd ? "Key stored · cached streams start instantly" : "Not set"}>
      <div class="field">
        API key
        <div class="row">
          <input
            type="password"
            bind:value={rdKey}
            placeholder={hasRd ? "Stored. Type to replace." : "Paste your key from real-debrid.com/apitoken"}
            autocomplete="off"
            spellcheck="false"
          />
          <button class="btn primary" disabled={!rdKey.trim()} onclick={() => saveRd(rdKey.trim())}>Save</button>
          {#if hasRd}<button class="btn danger" onclick={() => saveRd("")}>Remove</button>{/if}
        </div>
        <span class="hint">Optional. With a key, streams cached in Real-Debrid play right away over HTTP. It never leaves the server.</span>
      </div>
    </Section>

    {#if !touchOnly}
      <Section id="keys" title="Keyboard shortcuts" icon={Keyboard} summary="Search, rows and the player">
        <div class="keys">
          {#each KEYS as [k, what] (k + what)}
            <div class="key-row"><span class="kbds">{#each k.split("  ·  ") as one, i (one)}{#if i}<span class="or">or</span>{/if}<kbd>{one}</kbd>{/each}</span><span class="what">{what}</span></div>
          {/each}
        </div>
      </Section>
    {/if}

    {#if conn.available}<ConnectionSettings />{/if}

    <Section id="account" title="Account" icon={UserRound} summary="Signed in as {session.user?.username}">
      <div class="sub">Avatar</div>
      <AvatarPicker />
      <div class="sub">Username</div>
      <div class="row">
        <input bind:value={newName} autocomplete="username" spellcheck="false" autocapitalize="off" maxlength="32" />
        <button class="btn" disabled={renaming || !newName.trim() || newName.trim() === session.user?.username} onclick={rename}>Rename</button>
      </div>
      {#if conn.on === "local"}<span class="app-hint">Running on this device: the username can only change on your server (this device picks it up when it syncs).</span>{/if}
      <div class="sub">Change password</div>
      <div class="grid2">
        <input type="password" placeholder="Current password" bind:value={oldPw} autocomplete="current-password" />
        <input type="password" placeholder="New password" bind:value={newPw} autocomplete="new-password" />
      </div>
      <div><button class="btn" disabled={!oldPw || !newPw} onclick={changePw}>Change password</button></div>
      {#if isRemoteShell}
        <div class="sub"><Server size={12} /> Server</div>
        <div class="row">
          <input value={location.host} readonly />
          <button class="btn" onclick={() => leaveServer()}>Change server</button>
        </div>
      {:else if pickServer}
        <div class="sub"><Server size={12} /> Server</div>
        <ServerAddresses />
      {/if}
      <div class="sub"><MonitorSmartphone size={12} /> Signed-in devices</div>
      <Devices />
      <div class="sep"></div>
      <div class="row">
        <button class="btn danger" onclick={signOut}><LogOut size={14} />{tv.on ? "Remove profile" : "Sign out"}</button>
        <button class="btn ghost" onclick={switchAccount}><Users size={14} />{tv.on ? "Switch profile" : "Switch account"}</button>
      </div>
    </Section>

    {#if isAndroid || isDesktop}
      <Section id="app" title="App" icon={isAndroid ? Smartphone : Monitor} summary={shell.version ? `Version ${shell.version}` : isAndroid ? "Android" : "Desktop"}>
        <div class="row">
          <span class="grow">Mythos for {isAndroid ? "Android" : "Windows"} {shell.version}</span>
          {#if shell.update?.status === "ready"}
            <button class="btn primary" onclick={updateNow}><Download size={14} />Update now</button>
          {:else}
            <button class="btn" onclick={lookForUpdate} disabled={checking || !!shell.update}>
              <RefreshCw size={14} class={checking || shell.update ? "spin" : ""} />Check for updates
            </button>
          {/if}
        </div>
        {#if shell.update}
          <span class="app-hint">
            {shell.update.status === "ready"
              ? isAndroid
                ? `Mythos ${shell.update.version} is ready. It installs by itself when you leave the app while nothing is playing.`
                : `Mythos ${shell.update.version} is ready. Update now restarts Mythos right where you are; otherwise it installs when you close it.`
              : `Downloading Mythos ${shell.update.version}… ${Math.round(shell.update.progress * 100)}%`}
          </span>
        {:else if upToDate}
          <span class="app-hint">You have the latest version.</span>
        {:else}
          <span class="app-hint">
            {isAndroid ? "The app updates by itself when you leave it while nothing is playing." : "Mythos updates by itself when you close it."}
          </span>
        {/if}
        {#if isAndroid}<Switch bind:checked={useServerUi} onchange={toggleServerUi}>Load the interface from the server (new versions without updating the app)</Switch>{/if}
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
