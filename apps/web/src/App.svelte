<script lang="ts">
  import { House, Library, PanelLeftClose, PanelLeftOpen, Search, Settings, ShieldCheck } from "@lucide/svelte";
  import { flip } from "svelte/animate";
  import { fade } from "svelte/transition";
  import Confirm from "./components/Confirm.svelte";
  import Logo from "./components/Logo.svelte";
  import Titlebar from "./components/Titlebar.svelte";
  import { confirmState } from "./lib/confirm.svelte";
  import { flipMs, ms, rise, toastIn } from "./lib/motion";
  import { isDesktop } from "./lib/platform";
  import { prefs, savePrefs } from "./lib/prefs.svelte";
  import { go, loadView, route, searchIntent, viewComponent, type ViewName } from "./lib/router.svelte";
  import { session, start } from "./lib/session.svelte";
  import { toasts } from "./lib/toast.svelte";
  // El login no se separa en otro archivo: es lo primero que ve quien no entro todavia.
  import Login from "./views/Login.svelte";

  start();
  // Mientras se revisa la sesion ya se baja el codigo de la vista que toca.
  loadView(route.view).catch(() => {});

  const nav: { id: ViewName; path: string; label: string; icon: typeof House; admin?: boolean }[] = [
    { id: "home", path: "/", label: "Home", icon: House },
    { id: "search", path: "/search", label: "Search", icon: Search },
    { id: "library", path: "/library", label: "Library", icon: Library },
    { id: "settings", path: "/settings", label: "Settings", icon: Settings },
    { id: "admin", path: "/admin", label: "Admin", icon: ShieldCheck, admin: true },
  ];
  const items = $derived(nav.filter((n) => !n.admin || session.user?.is_admin));

  const View = $derived(viewComponent(route.view));
  const playing = $derived(route.view === "player");

  function toggleSidebar() {
    prefs.sidebarCollapsed = !prefs.sidebarCollapsed;
    savePrefs(false);
  }

  function typing(e: KeyboardEvent) {
    const t = e.target as HTMLElement | null;
    return !!t?.closest("input, textarea, select, [contenteditable]");
  }

  function onKey(e: KeyboardEvent) {
    if (!session.user || playing || confirmState.open) return;
    // La busqueda todavia se esta abriendo: lo que se escribe no se pierde.
    if (searchIntent.pending && !typing(e)) {
      if (e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) {
        e.preventDefault();
        searchIntent.typed += e.key;
      } else if (e.key === "Backspace") {
        e.preventDefault();
        searchIntent.typed = searchIntent.typed.slice(0, -1);
      }
      return;
    }
    // "/" o Ctrl K: a la busqueda (y al campo, si ya esta ahi).
    if ((e.key === "/" && !typing(e)) || ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k")) {
      e.preventDefault();
      if (route.view === "search") document.querySelector<HTMLInputElement>("#search-input")?.focus();
      else {
        searchIntent.pending = true;
        searchIntent.typed = "";
        go("/search");
        // Por las dudas (sin red, la vista no carga): no seguir tragando teclas.
        setTimeout(() => (searchIntent.pending = false), 5000);
      }
    }
  }

  // Sin menu contextual del navegador fuera de los campos de texto (en desktop se nota "web").
  function onContext(e: MouseEvent) {
    if (isDesktop && !(e.target as HTMLElement).closest("input, textarea")) e.preventDefault();
  }
</script>

<svelte:window onkeydown={onKey} oncontextmenu={onContext} />

<div class="frame">
  {#if !session.checked}
    {#if isDesktop}<Titlebar bare />{/if}
    <div class="splash" out:fade={{ duration: ms(200) }}>
      <Logo size={64} shine />
    </div>
  {:else if !session.user}
    {#if isDesktop}<Titlebar bare />{/if}
    <div class="body"><Login /></div>
  {:else if playing}
    <div class="body player-host">
      {#if View}
        {#key route.key}<View params={route.params} query={route.query} />{/key}
      {:else}
        <div class="splash dark"><Logo size={48} shine /></div>
      {/if}
    </div>
  {:else}
    {#if isDesktop}<Titlebar ontoggle={toggleSidebar} />{/if}
    <div class="body shell" class:collapsed={prefs.sidebarCollapsed}>
      <aside class="sidebar">
        {#if !isDesktop}
          <button class="brand" onclick={() => go("/")} aria-label="Home">
            <Logo size={26} />
            <span class="label word">Mythos</span>
          </button>
        {/if}
        <nav>
          {#each items as n (n.id)}
            <a
              class="nav"
              class:on={route.view === n.id || (n.id === "home" && route.view === "detail")}
              href="#{n.path}"
              title={n.label}
              onclick={(e) => {
                e.preventDefault();
                go(n.path);
              }}
              onpointerenter={() => loadView(n.id).catch(() => {})}
            >
              <n.icon size={18} />
              <span class="label">{n.label}</span>
            </a>
          {/each}
        </nav>
        <div class="grow"></div>
        <div class="me label ellipsis" title={session.user.username}>
          <span class="avatar">{session.user.username.slice(0, 1).toUpperCase()}</span>
          <span class="ellipsis">{session.user.username}</span>
        </div>
        <button class="nav collapse" onclick={toggleSidebar} title={prefs.sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar"}>
          {#if prefs.sidebarCollapsed}<PanelLeftOpen size={18} />{:else}<PanelLeftClose size={18} />{/if}
          <span class="label">Collapse</span>
        </button>
      </aside>

      <main>
        {#key route.key}
          <div class="view-page" in:rise={{ y: 6, skip: route.vt }}>
            {#if View}
              <View params={route.params} query={route.query} />
            {:else}
              <div class="loading-view">
                <div class="skel" style="height: 38vh; border-radius: var(--radius)"></div>
                <div class="skel line" style="width: 160px; margin-top: 28px"></div>
                <div class="skel-row">
                  {#each Array(7) as _, i (i)}<div class="skel poster"></div>{/each}
                </div>
              </div>
            {/if}
          </div>
        {/key}
      </main>
    </div>
  {/if}
</div>

{#if confirmState.open}
  <Confirm />
{/if}

<div class="toasts" class:raised={!playing}>
  {#each toasts as t (t.id)}
    <div class="toast card" class:bad={t.bad} in:toastIn out:fade={{ duration: ms(150) }} animate:flip={{ duration: flipMs() }}>
      {t.text}
    </div>
  {/each}
</div>

<style>
  .frame {
    height: 100%;
    display: flex;
    flex-direction: column;
  }
  .body {
    flex: 1;
    min-height: 0;
    position: relative;
  }
  .splash {
    flex: 1;
    display: grid;
    place-items: center;
  }
  .splash.dark {
    height: 100%;
    background: var(--media-bg);
  }
  .player-host {
    background: var(--media-bg);
  }
  .shell {
    --side-w: 212px;
    display: grid;
    grid-template-columns: var(--side-w) 1fr;
    transition: grid-template-columns 0.26s cubic-bezier(0.2, 0.8, 0.2, 1);
  }
  .shell.collapsed {
    --side-w: 64px;
  }
  .sidebar {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 14px 10px 12px;
    background: var(--sidebar);
    backdrop-filter: var(--blur);
    border-right: var(--border) solid var(--line);
    min-height: 0;
    overflow: hidden;
    z-index: 10;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 40px;
    padding: 0 9px;
    margin-bottom: 14px;
  }
  .word {
    font-family: var(--display);
    font-weight: 700;
    font-size: 17px;
    letter-spacing: -0.02em;
  }
  nav {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .nav {
    display: flex;
    align-items: center;
    gap: 12px;
    height: 38px;
    padding: 0 12px;
    border-radius: var(--radius-sm);
    color: var(--text-2);
    font-weight: 540;
    white-space: nowrap;
    transition:
      background 0.15s,
      color 0.15s;
  }
  .nav :global(svg) {
    flex: none;
    transition: transform 0.2s cubic-bezier(0.34, 1.56, 0.64, 1);
  }
  .nav:hover {
    background: var(--hover);
    color: var(--text);
  }
  .nav:hover :global(svg) {
    transform: scale(1.08);
  }
  .nav:active :global(svg) {
    transform: scale(0.92);
  }
  .nav.on {
    background: var(--panel-2);
    color: var(--text);
    box-shadow: inset 2px 0 0 var(--accent);
  }
  .nav.on :global(svg) {
    color: var(--accent);
  }
  :global([data-theme="poolsuite"]) .nav.on {
    background: var(--accent);
    color: var(--accent-ink);
    box-shadow: 2px 2px 0 var(--line);
    border: 2px solid var(--line);
  }
  :global([data-theme="poolsuite"]) .nav.on :global(svg) {
    color: var(--accent-ink);
  }
  .label {
    transition: opacity 0.18s;
  }
  .collapsed .label {
    opacity: 0;
    pointer-events: none;
  }
  .me {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 8px;
    color: var(--text-2);
    font-size: 12.5px;
  }
  .avatar {
    flex: none;
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
  .collapsed .me {
    opacity: 1;
  }
  .collapsed .me > span:last-child {
    opacity: 0;
  }
  .collapse {
    color: var(--text-3);
  }
  main {
    min-width: 0;
    min-height: 0;
    position: relative;
  }
  .view-page {
    position: absolute;
    inset: 0;
  }
  .loading-view {
    padding: 22px var(--gutter);
  }
  .skel-row {
    display: flex;
    gap: 14px;
    margin-top: 14px;
    overflow: hidden;
  }
  .skel.poster {
    width: 150px;
    flex: none;
    aspect-ratio: 2 / 3;
    border-radius: var(--radius);
  }
  .toasts {
    position: fixed;
    right: 16px;
    bottom: 16px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    z-index: 120;
    max-width: min(420px, calc(100vw - 32px));
  }
  .toast {
    padding: 11px 15px;
    background: var(--sidebar);
    box-shadow: var(--pop-shadow);
    font-size: 13px;
    user-select: text;
  }
  .toast.bad {
    border-color: var(--danger);
    color: var(--danger);
  }

  /* Celular: la barra lateral pasa a ser una barra abajo, solo iconos. */
  @media (max-width: 700px) {
    .shell,
    .shell.collapsed {
      grid-template-columns: 1fr;
      grid-template-rows: 1fr auto;
    }
    main {
      grid-row: 1;
    }
    .sidebar {
      grid-row: 2;
      flex-direction: row;
      padding: 6px 8px calc(6px + env(safe-area-inset-bottom));
      border-right: 0;
      border-top: var(--border) solid var(--line);
    }
    .brand,
    .me,
    .collapse,
    .grow {
      display: none;
    }
    nav {
      flex: 1;
      flex-direction: row;
      justify-content: space-around;
    }
    .nav {
      flex-direction: column;
      gap: 3px;
      height: 50px;
      padding: 6px 10px 4px;
      font-size: 10.5px;
    }
    .nav.on {
      box-shadow: inset 0 2px 0 var(--accent);
    }
    .label,
    .collapsed .label {
      opacity: 1;
    }
    .toasts.raised {
      bottom: 76px;
    }
  }
</style>
