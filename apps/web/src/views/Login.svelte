<script lang="ts">
  // Login: centrado, con la marca que brilla. Contraseña incorrecta: la tarjeta tiembla.
  // En desktop y en la pagina del APK de Android se elige ademas el server (campo desplegable,
  // abierto la primera vez), con una o mas direcciones del mismo server (Tailscale, la red de
  // casa: lib/servers.svelte.ts). En Android con la interfaz del server, "Change server" vuelve a la
  // pagina del APK.
  import { ChevronDown, LoaderCircle, Plus, Server, X } from "@lucide/svelte";
  import Logo from "../components/Logo.svelte";
  import { api, ApiError, errText } from "../lib/api";
  import { expand } from "../lib/motion";
  import { checkServer, DEFAULT_SERVER, isAddress, isRemoteShell, needsServer, normalizeServer, pickServer } from "../lib/platform";
  import { isOurs, servers, setAddresses } from "../lib/servers.svelte";
  import { viaLocal } from "../lib/local.svelte";
  import { login, session } from "../lib/session.svelte";
  import { profiles } from "../lib/profiles.svelte";
  import { leaveServer } from "../lib/shell.svelte";
  import { tv } from "../lib/tv.svelte";

  let username = $state("");
  let password = $state("");
  // Las direcciones del server (la primera es la principal; las demas, opcionales).
  let addrs = $state<string[]>(servers.list.length ? servers.list.map((a) => a.url) : [DEFAULT_SERVER]);
  const shownAddr = $derived.by(() => {
    const filled = addrs.map((a) => a.trim()).filter(Boolean);
    if (!filled.length) return "Choose a server";
    return filled[0].replace(/^https?:\/\//, "") + (filled.length > 1 ? ` +${filled.length - 1}` : "");
  });
  // Abierto si falta, o si se vino a cambiarlo (#/server).
  let showServer = $state(needsServer() || location.hash.startsWith("#/server"));
  let error = $state("");
  let busy = $state(false);
  let shake = $state(false);
  let userField = $state<HTMLInputElement>();

  $effect(() => {
    // En pantallas tactiles no: el teclado taparia la mitad del login apenas se abre. En la TV
    // tampoco: el foco lo pone el control (y el teclado se abre con el centro).
    if (!matchMedia("(pointer: coarse)").matches && !tv.on) userField?.focus();
  });

  async function submit(e: Event) {
    e.preventDefault();
    error = "";
    busy = true;
    if (pickServer) {
      const bad = await checkAddrs();
      if (bad) {
        error = bad;
        showServer = true;
        busy = false;
        return wobble();
      }
    }
    try {
      await login(username.trim(), password);
    } catch (err) {
      // Sin server (login contra la copia del dispositivo): solo cuentas que ya entraron aca.
      if (err instanceof ApiError && err.status === 401 && err.message.includes("offline")) error = "Wrong username or password. Without the server, only accounts that already signed in on this device can sign in.";
      else if (err instanceof ApiError && err.status === 401) error = "Wrong username or password.";
      else if (err instanceof ApiError && err.status === 429) error = "Too many attempts. Wait a minute and try again.";
      else {
        error = errText(err);
        // Sin conexion: en desktop probablemente es la direccion del server.
        if (pickServer) showServer = true;
        api.health().catch(() => (error = pickServer ? "Can't reach the server. Check the address." : "Can't reach the server."));
      }
      password = "";
      wobble();
    } finally {
      busy = false;
    }
  }

  /** Antes de mandar la contraseña: ¿hay un Mythos en esas direcciones, y es el mismo en todas? Las
   *  que no contestan se guardan igual (p. ej. la de casa estando afuera): se prueban cuando
   *  contesten. Devuelve el error para mostrar, o "". */
  async function checkAddrs(): Promise<string> {
    const urls = [...new Set(addrs.map(normalizeServer).filter(Boolean))];
    if (!urls.length) return needsServer() ? "Enter the address of your Mythos server." : "";
    const wrong = urls.find((u) => !isAddress(u));
    if (wrong) return (await checkServer(wrong)).error;
    const checks = await Promise.all(urls.map((u) => checkServer(u)));
    const ok = checks.filter((c) => c.probe);
    if (!ok.length) {
      // Desktop y APK con Auto / Solo local: sin server se puede entrar igual, contra la copia de
      // la cuenta que guarda el dispositivo (si ya se entro una vez con el server).
      if (urls.every(isOurs) && (await viaLocal())) return "";
      // "Es el server de una app" dice mas que "no contesta".
      return checks.find((c) => !c.error.startsWith("Can't"))?.error ?? checks[0].error;
    }
    if (new Set(ok.map((c) => c.probe!.id).filter(Boolean)).size > 1) return "These addresses lead to different Mythos servers. Keep only the addresses of your server.";
    setAddresses(urls.map((url, i) => ({ url, probe: checks[i].probe })));
    addrs = urls;
    return "";
  }

  function wobble() {
    shake = false;
    requestAnimationFrame(() => (shake = true));
  }
</script>

<div class="wrap">
  <div class="aurora" aria-hidden="true"><span></span><span></span><span></span></div>
  <!-- El temblor va en un envoltorio propio (como en Ethereon): puesto en la tarjeta reemplazaba
       su animacion de entrada. -->
  <div class="shaker" class:shake onanimationend={(e) => e.animationName === "shake" && (shake = false)}>
    <form class="card box stagger" onsubmit={submit}>
      <div class="mark"><Logo size={56} shine /></div>
      <h1>{tv.on && profiles.adding ? "Add a profile" : "Welcome to Mythos"}</h1>
      {#if tv.on && profiles.adding}
        <p class="muted">Sign in once. This TV stays signed in to this account until you remove it.</p>
      {:else if session.expired}
        <p class="notice">Your session expired. Sign in again to continue.</p>
      {:else}
        <p class="muted">Sign in to keep watching.</p>
      {/if}
      <label class="field">
        Username
        <!-- data-autofocus: en la TV el foco arranca aca (lib/spatial.svelte.ts). -->
        <input bind:this={userField} data-autofocus autocomplete="username" bind:value={username} spellcheck="false" autocapitalize="off" />
      </label>
      <label class="field">
        Password
        <input type="password" autocomplete="current-password" bind:value={password} />
      </label>

      {#if isRemoteShell}
        <button type="button" class="srv-toggle" onclick={() => leaveServer()}>
          <Server size={13} />
          <span class="ellipsis">{location.host} · Change</span>
        </button>
      {:else if pickServer}
        <button type="button" class="srv-toggle" class:open={showServer} onclick={() => (showServer = !showServer)}>
          <Server size={13} />
          <span class="ellipsis">{shownAddr}</span>
          <ChevronDown size={14} />
        </button>
        {#if showServer}
          <div class="addrs" transition:expand>
            {#each addrs as _, i (i)}
              <label class="field">
                {i === 0 ? "Server URL" : "Another address of the same server"}
                <span class="addr">
                  <input
                    bind:value={addrs[i]}
                    inputmode="url"
                    placeholder={i === 0 ? "https://your-server.ts.net:7700" : "http://192.168.1.20:7700"}
                    spellcheck="false"
                    autocapitalize="off"
                  />
                  {#if addrs.length > 1}
                    <button type="button" class="icon-btn" title="Remove this address" aria-label="Remove this address" onclick={() => addrs.splice(i, 1)}><X size={14} /></button>
                  {/if}
                </span>
                {#if i === addrs.length - 1}
                  <span class="hint">
                    {addrs.length > 1
                      ? "Mythos uses whichever address answers fastest. You can change them later in Settings → Account."
                      : "The address of your Mythos server, for example https://my-server.ts.net:7700"}
                  </span>
                {/if}
              </label>
            {/each}
            <button type="button" class="btn ghost sm add" onclick={() => addrs.push("")}><Plus size={13} />Add another address</button>
          </div>
        {/if}
      {/if}

      {#if error}<div class="err" role="alert">{error}</div>{/if}
      <button class="btn primary big" disabled={busy || !username.trim() || !password}>
        {#if busy}<LoaderCircle size={16} class="spin" />{/if}
        Sign in
      </button>
      {#if tv.on && profiles.list.length}
        <button type="button" class="btn ghost" onclick={() => ((profiles.adding = false), (profiles.picking = true))}>Back to profiles</button>
      {/if}
      <div class="faint small">No account? Ask the admin of this server.</div>
    </form>
  </div>
</div>

<style>
  .wrap {
    position: relative;
    height: 100%;
    display: grid;
    place-items: center;
    padding: 20px;
    overflow: hidden;
  }
  /* Tres manchas del acento que se mueven despacio detras de la tarjeta. */
  /* Sin filter: blur (re-dibujaria toda la capa en cada cuadro); los degradados ya son suaves y
     el movimiento es solo transform. */
  .aurora {
    position: absolute;
    inset: 0;
    pointer-events: none;
    opacity: 0.6;
  }
  .aurora span {
    position: absolute;
    width: 42vmax;
    height: 42vmax;
    border-radius: 50%;
    background: radial-gradient(circle, color-mix(in srgb, var(--accent) 22%, transparent), transparent 62%);
    animation: drift 18s ease-in-out infinite alternate;
    will-change: transform;
  }
  .aurora span:nth-child(1) {
    left: -10vmax;
    top: -14vmax;
  }
  .aurora span:nth-child(2) {
    right: -14vmax;
    bottom: -18vmax;
    animation-duration: 23s;
    animation-delay: -6s;
    background: radial-gradient(circle, color-mix(in srgb, var(--accent) 18%, var(--text) 6%), transparent 65%);
  }
  .aurora span:nth-child(3) {
    left: 40%;
    top: 55%;
    width: 26vmax;
    height: 26vmax;
    animation-duration: 29s;
    animation-delay: -12s;
  }
  @keyframes drift {
    to {
      transform: translate(8vmax, 6vmax) scale(1.15);
    }
  }
  :global([data-theme="poolsuite"]) .aurora,
  :global([data-theme="athanor"]) .aurora {
    display: none;
  }
  .shaker {
    position: relative;
    width: min(380px, 100%);
  }
  .box {
    width: 100%;
    padding: 30px 28px 24px;
    display: flex;
    flex-direction: column;
    gap: 13px;
    box-shadow: var(--pop-shadow);
    background: var(--sidebar);
  }
  :global([data-theme="warp"]) .box,
  :global([data-theme="haze"]) .box {
    background: color-mix(in srgb, var(--bg) 70%, transparent);
    backdrop-filter: blur(24px);
  }
  .mark {
    display: grid;
    place-items: center;
    margin: 2px 0 4px;
  }
  h1 {
    margin: 0;
    font-family: var(--display);
    font-size: 21px;
    font-weight: 700;
    letter-spacing: -0.02em;
    text-align: center;
  }
  p {
    margin: -6px 0 6px;
    text-align: center;
    font-size: 13px;
  }
  .notice {
    padding: 8px 10px;
    border-radius: var(--radius-sm);
    color: var(--text);
    background: color-mix(in srgb, var(--warn) 14%, transparent);
    border: var(--border) solid color-mix(in srgb, var(--warn) 40%, transparent);
  }
  .big {
    height: 40px;
    margin-top: 4px;
  }
  .err {
    color: var(--danger);
    font-size: 12.5px;
  }
  .small {
    font-size: 11.5px;
    text-align: center;
  }
  .addrs {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .addr {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .addr input {
    flex: 1;
    min-width: 0;
  }
  .add {
    align-self: flex-start;
    margin: -4px 0 0 -9px;
    color: var(--text-3);
  }
  .srv-toggle {
    display: flex;
    align-items: center;
    gap: 7px;
    align-self: flex-start;
    max-width: 100%;
    padding: 3px 8px;
    margin: -2px 0 -4px -8px;
    border-radius: var(--radius-sm);
    color: var(--text-3);
    font-size: 12px;
    transition:
      color 0.15s,
      background 0.15s;
  }
  .srv-toggle:hover {
    color: var(--text-2);
    background: var(--hover);
  }
  .srv-toggle :global(svg:last-child) {
    flex: none;
    transition: transform 0.2s;
  }
  .srv-toggle.open :global(svg:last-child) {
    transform: rotate(180deg);
  }
</style>
