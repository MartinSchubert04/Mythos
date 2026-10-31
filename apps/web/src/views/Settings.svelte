<script lang="ts">
  // Ajustes: tema y paleta con muestras en vivo (como Ethereon), reproduccion, subtitulos,
  // Real-Debrid y la cuenta. Lo de la interfaz se guarda solo (local y en el server).
  import { Captions, KeyRound, LogOut, Palette, Play, Server, UserRound } from "@lucide/svelte";
  import Section from "../components/Section.svelte";
  import Seg from "../components/Seg.svelte";
  import Select from "../components/Select.svelte";
  import Switch from "../components/Switch.svelte";
  import { api } from "../lib/api";
  import { ask } from "../lib/confirm.svelte";
  import { isDesktop, serverSetting, setServer } from "../lib/platform";
  import { LANGS, prefs, savePrefs } from "../lib/prefs.svelte";
  import { keepScroll } from "../lib/scroll";
  import { logout, session } from "../lib/session.svelte";
  import { THEMES, applyTheme, currentPalette, currentTheme, palettesOf, type ThemeId } from "../lib/themes";
  import { attempt } from "../lib/toast.svelte";
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
  async function signOut() {
    if (await ask({ title: "Sign out?", body: "You'll need your password to come back.", confirm: "Sign out" })) logout();
  }

  let server = $state(serverSetting());
  async function saveServerUrl() {
    setServer(server);
    if (await ask({ title: "Switch server?", body: "Mythos will sign you out to connect to the new address.", confirm: "Switch" })) {
      await logout();
      location.reload();
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
      </div>
      <Switch bind:checked={prefs.autoplayNext} onchange={() => savePrefs()}>Play the next episode automatically (10 second countdown)</Switch>
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

    <Section id="account" title="Account" icon={UserRound} summary="Signed in as {session.user?.username}">
      <div class="sub">Change password</div>
      <div class="grid2">
        <input type="password" placeholder="Current password" bind:value={oldPw} autocomplete="current-password" />
        <input type="password" placeholder="New password" bind:value={newPw} autocomplete="new-password" />
      </div>
      <div><button class="btn" disabled={!oldPw || !newPw} onclick={changePw}>Change password</button></div>
      {#if isDesktop}
        <div class="sub"><Server size={12} /> Server</div>
        <div class="row">
          <input bind:value={server} placeholder="https://your-server.ts.net:7700" spellcheck="false" />
          <button class="btn" disabled={server.trim() === serverSetting()} onclick={saveServerUrl}>Save</button>
        </div>
      {/if}
      <div class="sep"></div>
      <div><button class="btn danger" onclick={signOut}><LogOut size={14} />Sign out</button></div>
    </Section>
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
    max-width: 900px;
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
  .sep {
    height: 1px;
    background: var(--line);
  }
  .row input {
    flex: 1;
  }
</style>
