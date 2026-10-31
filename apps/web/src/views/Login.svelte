<script lang="ts">
  // Login: centrado, con la marca que brilla. Contraseña incorrecta: la tarjeta tiembla.
  // En desktop se elige ademas el server (campo desplegable).
  import { ChevronDown, LoaderCircle, Server } from "@lucide/svelte";
  import Logo from "../components/Logo.svelte";
  import { api, ApiError, errText } from "../lib/api";
  import { expand } from "../lib/motion";
  import { isDesktop, needsServer, serverSetting, setServer } from "../lib/platform";
  import { login } from "../lib/session.svelte";

  let username = $state("");
  let password = $state("");
  let server = $state(serverSetting());
  let showServer = $state(needsServer());
  let error = $state("");
  let busy = $state(false);
  let shake = $state(false);
  let userField = $state<HTMLInputElement>();

  $effect(() => {
    userField?.focus();
  });

  async function submit(e: Event) {
    e.preventDefault();
    error = "";
    if (isDesktop) {
      if (!server.trim() && needsServer()) {
        error = "Enter the address of your Mythos server.";
        showServer = true;
        return wobble();
      }
      setServer(server);
    }
    busy = true;
    try {
      await login(username.trim(), password);
    } catch (err) {
      if (err instanceof ApiError && err.status === 401) error = "Wrong username or password.";
      else if (err instanceof ApiError && err.status === 429) error = "Too many attempts. Wait a minute and try again.";
      else {
        error = errText(err);
        // Sin conexion: en desktop probablemente es la direccion del server.
        if (isDesktop) showServer = true;
        api.health().catch(() => (error = isDesktop ? "Can't reach the server. Check the address." : "Can't reach the server."));
      }
      password = "";
      wobble();
    } finally {
      busy = false;
    }
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
      <h1>Welcome to Mythos</h1>
      <p class="muted">Sign in to keep watching.</p>
      <label class="field">
        Username
        <input bind:this={userField} autocomplete="username" bind:value={username} spellcheck="false" autocapitalize="off" />
      </label>
      <label class="field">
        Password
        <input type="password" autocomplete="current-password" bind:value={password} />
      </label>

      {#if isDesktop}
        <button type="button" class="srv-toggle" class:open={showServer} onclick={() => (showServer = !showServer)}>
          <Server size={13} />
          <span class="ellipsis">{server.trim() ? server.replace(/^https?:\/\//, "") : "Choose a server"}</span>
          <ChevronDown size={14} />
        </button>
        {#if showServer}
          <div transition:expand>
            <label class="field">
              Server
              <input bind:value={server} placeholder="https://your-server.ts.net:7700" spellcheck="false" autocapitalize="off" />
              <span class="hint">The address of your Mythos server, for example https://my-server.ts.net:7700</span>
            </label>
          </div>
        {/if}
      {/if}

      {#if error}<div class="err" role="alert">{error}</div>{/if}
      <button class="btn primary big" disabled={busy || !username.trim() || !password}>
        {#if busy}<LoaderCircle size={16} class="spin" />{/if}
        Sign in
      </button>
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
  .aurora {
    position: absolute;
    inset: 0;
    pointer-events: none;
    filter: blur(70px);
    opacity: 0.55;
  }
  .aurora span {
    position: absolute;
    width: 42vmax;
    height: 42vmax;
    border-radius: 50%;
    background: radial-gradient(circle, color-mix(in srgb, var(--accent) 30%, transparent), transparent 65%);
    animation: drift 18s ease-in-out infinite alternate;
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
