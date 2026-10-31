<script lang="ts">
  // Settings → Account → Server (desktop y la pagina del APK): las direcciones del server de casa
  // en este dispositivo. Todas llevan al mismo server (p. ej. Tailscale y la red de casa); se usa
  // la preferida (estrella) si contesta, si no la mas rapida. Cada una con su latencia y un nombre
  // opcional (si no, se deduce: "Tailscale", "Home network"). Una direccion de otro server ofrece
  // cambiar de server (sale de la cuenta). La logica esta en lib/servers.svelte.ts.
  import { flip } from "svelte/animate";
  import { Plus, RefreshCw, Star, X } from "@lucide/svelte";
  import { ask } from "../lib/confirm.svelte";
  import { flipMs } from "../lib/motion";
  import { addrLabel, checkServer, normalizeServer } from "../lib/platform";
  import { addAddress, choose, removeAddress, servers, setAddresses, setLabel, setPreferred, type Ping } from "../lib/servers.svelte";
  import { logout } from "../lib/session.svelte";
  import { toast } from "../lib/toast.svelte";

  let adding = $state("");
  let busy = $state(false);
  let err = $state("");

  // Al abrir: la latencia de cada una (y, si hay una mejor, pasa a esa).
  choose("manual");

  function pingText(p: Ping | undefined): string {
    if (p === undefined) return servers.probing ? "Testing…" : "Not tested";
    if (p === null) return "No response";
    if (p === "other") return "Different server";
    if (p === "local") return "Not your server";
    return `${p} ms`;
  }

  async function add(e: Event) {
    e.preventDefault();
    const url = normalizeServer(adding);
    err = "";
    if (!url) return;
    if (servers.list.some((a) => a.url === url)) {
      err = "That address is already on the list.";
      return;
    }
    busy = true;
    const { probe, error } = await checkServer(url);
    busy = false;
    // Mal escrita, o el server de una app: no.
    if (!probe && !error.startsWith("Can't")) {
      err = error;
      return;
    }
    const known = servers.list.find((a) => a.id)?.id;
    if (probe?.id && known && probe.id !== known) {
      const ok = await ask({
        title: "Switch to this server?",
        body: "This address leads to a different Mythos server. Mythos will sign you out and use only this address.",
        confirm: "Switch server",
      });
      if (ok) {
        setAddresses([{ url, probe }]);
        await logout();
        location.reload();
      }
      return;
    }
    if (!probe) {
      const ok = await ask({
        title: "Add it anyway?",
        body: `${url.replace(/^https?:\/\//, "")} isn't answering right now (for example, it's your home address and you're away). Mythos starts using it when it answers, after checking that it's the same server.`,
        confirm: "Add address",
      });
      if (!ok) return;
    }
    addAddress(url, probe);
    adding = "";
    toast(`Added ${addrLabel(url)}`);
    if (probe) choose("manual");
  }

  async function remove(url: string) {
    if (await ask({ title: "Remove this address?", body: url.replace(/^https?:\/\//, ""), confirm: "Remove", danger: true })) removeAddress(url);
  }
</script>

<div class="addrs">
  {#each servers.list as a (a.url)}
    {@const p = servers.ping[a.url]}
    {@const pref = servers.preferred === a.url}
    <div class="addr" class:on={a.url === servers.active} animate:flip={{ duration: flipMs() }}>
      <span class="dot" class:ok={typeof p === "number"} class:bad={p === null || p === "other" || p === "local"}></span>
      <div class="info">
        <input
          class="name"
          value={a.label ?? ""}
          placeholder={addrLabel(a.url)}
          maxlength="40"
          spellcheck="false"
          aria-label="Name of this address"
          title="Name of this address"
          onchange={(e) => setLabel(a.url, e.currentTarget.value)}
        />
        <div class="sub">
          <span class="mono ellipsis" title={a.url}>{a.url.replace(/^https?:\/\//, "")}</span>
          <span class:warn={p === "other" || p === "local"}>{pingText(p)}</span>
        </div>
      </div>
      {#if a.url === servers.active && servers.list.length > 1}<span class="chip accent">In use</span>{/if}
      {#if servers.list.length > 1}
        <button
          class="icon-btn"
          class:pref
          title={pref ? "Preferred: used whenever it answers. Click to stop preferring it." : "Prefer this address whenever it answers"}
          aria-label={pref ? "Stop preferring this address" : "Prefer this address"}
          aria-pressed={pref}
          onclick={() => setPreferred(a.url)}
        >
          <Star size={15} fill={pref ? "currentColor" : "none"} />
        </button>
        <button class="icon-btn del" title="Remove this address" aria-label="Remove this address" onclick={() => remove(a.url)}><X size={15} /></button>
      {/if}
    </div>
  {/each}
  <form class="row" onsubmit={add}>
    <input bind:value={adding} inputmode="url" placeholder="Add another address, e.g. http://192.168.1.20:7700" spellcheck="false" autocapitalize="off" />
    <button class="btn" disabled={busy || !adding.trim()}><Plus size={14} />{busy ? "Checking…" : "Add"}</button>
    <button type="button" class="btn ghost" disabled={servers.probing} onclick={() => choose("manual")} title="Test every address again and use the best one">
      <RefreshCw size={14} class={servers.probing ? "spin" : ""} />Test
    </button>
  </form>
  {#if err}<div class="err">{err}</div>{/if}
  <span class="hint">
    Every address must lead to this same server, for example its Tailscale address and its address at home. Mythos uses the starred one whenever it answers,
    otherwise the fastest, and switches by itself when the network changes. These addresses are saved on this device only.
  </span>
</div>

<style>
  .addrs {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .addr {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto auto auto;
    align-items: center;
    gap: 10px;
    padding: 7px 8px 7px 12px;
    border: var(--border) solid var(--line);
    border-radius: var(--radius-sm);
    background: var(--panel-2);
  }
  .addr.on {
    border-color: color-mix(in srgb, var(--accent) 45%, var(--line));
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--text-3);
  }
  .dot.ok {
    background: var(--ok);
  }
  .dot.bad {
    background: var(--danger);
  }
  .info {
    min-width: 0;
  }
  /* El nombre se edita en el lugar: parece texto hasta que se lo toca. */
  .name {
    width: 100%;
    height: 24px;
    padding: 0 6px;
    margin-left: -6px;
    font-weight: 600;
    background: transparent;
    border-color: transparent;
    box-shadow: none;
  }
  .name:hover,
  .name:focus-visible {
    background: var(--panel);
    border-color: var(--line);
  }
  .sub {
    display: flex;
    gap: 10px;
    min-width: 0;
    font-size: 12px;
    color: var(--text-3);
  }
  .sub .mono {
    font-size: 11.5px;
    min-width: 0;
  }
  .sub span:last-child {
    flex: none;
  }
  .warn {
    color: var(--warn);
  }
  .pref {
    color: var(--accent);
  }
  .del:hover {
    color: var(--danger);
  }
  .row {
    display: flex;
    gap: 8px;
    margin-top: 4px;
  }
  .row input {
    flex: 1;
    min-width: 0;
  }
  .err {
    color: var(--danger);
    font-size: 12.5px;
  }
  .hint {
    font-size: 12.5px;
    color: var(--text-3);
    line-height: 1.5;
  }
  @media (max-width: 560px) {
    .row {
      flex-wrap: wrap;
    }
    .row input {
      flex-basis: 100%;
    }
  }
</style>
