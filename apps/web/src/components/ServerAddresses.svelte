<script lang="ts">
  // Settings → Account → Server (desktop y la pagina del APK): las direcciones del server de casa
  // en este dispositivo. Todas llevan al mismo server (p. ej. Tailscale y la red de casa); se usa
  // la preferida (estrella) si contesta, si no la mas rapida. Cada una con su latencia y un nombre
  // opcional (si no, se deduce: "Tailscale", "Home network"). Una direccion de otro server ofrece
  // cambiar de server (sale de la cuenta). La logica esta en lib/servers.svelte.ts.
  import { flip } from "svelte/animate";
  import { Plus, RefreshCw, Star, X } from "@lucide/svelte";
  import { ask } from "../lib/confirm.svelte";
  import { t } from "../lib/i18n.svelte";
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
    if (p === undefined) return servers.probing ? t("account.addr.testing") : t("account.addr.notTested");
    if (p === null) return t("account.addr.noResponse");
    if (p === "other") return t("account.addr.different");
    if (p === "local") return t("account.addr.notYours");
    return `${p} ms`;
  }

  async function add(e: Event) {
    e.preventDefault();
    const url = normalizeServer(adding);
    err = "";
    if (!url) return;
    if (servers.list.some((a) => a.url === url)) {
      err = t("account.addr.dup");
      return;
    }
    busy = true;
    const { probe, error, down } = await checkServer(url);
    busy = false;
    // Mal escrita, o el server de una app: no.
    if (!probe && !down) {
      err = error;
      return;
    }
    const known = servers.list.find((a) => a.id)?.id;
    if (probe?.id && known && probe.id !== known) {
      const ok = await ask({
        title: t("account.addr.switchTitle"),
        body: t("account.addr.switchBody"),
        confirm: t("account.addr.switchConfirm"),
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
        title: t("account.addr.anywayTitle"),
        body: t("account.addr.anywayBody", { host: url.replace(/^https?:\/\//, "") }),
        confirm: t("account.addr.anywayConfirm"),
      });
      if (!ok) return;
    }
    addAddress(url, probe);
    adding = "";
    toast(t("account.addr.added", { name: addrLabel(url) }));
    if (probe) choose("manual");
  }

  async function remove(url: string) {
    if (await ask({ title: t("account.addr.removeTitle"), body: url.replace(/^https?:\/\//, ""), confirm: t("common.remove"), danger: true })) removeAddress(url);
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
          aria-label={t("account.addr.name")}
          title={t("account.addr.name")}
          onchange={(e) => setLabel(a.url, e.currentTarget.value)}
        />
        <div class="sub">
          <span class="mono ellipsis" title={a.url}>{a.url.replace(/^https?:\/\//, "")}</span>
          <span class:warn={p === "other" || p === "local"}>{pingText(p)}</span>
        </div>
      </div>
      {#if a.url === servers.active && servers.list.length > 1}<span class="chip accent">{t("account.addr.inUse")}</span>{/if}
      {#if servers.list.length > 1}
        <button
          class="icon-btn"
          class:pref
          title={pref ? t("account.addr.prefOnHint") : t("account.addr.prefOffHint")}
          aria-label={pref ? t("account.addr.prefOn") : t("account.addr.prefOff")}
          aria-pressed={pref}
          onclick={() => setPreferred(a.url)}
        >
          <Star size={15} fill={pref ? "currentColor" : "none"} />
        </button>
        <button class="icon-btn del" title={t("account.addr.removeOne")} aria-label={t("account.addr.removeOne")} onclick={() => remove(a.url)}><X size={15} /></button>
      {/if}
    </div>
  {/each}
  <form class="row" onsubmit={add}>
    <input bind:value={adding} inputmode="url" placeholder={t("account.addr.addPlaceholder")} spellcheck="false" autocapitalize="off" />
    <button class="btn" disabled={busy || !adding.trim()}><Plus size={14} />{busy ? t("account.addr.checking") : t("account.addr.add")}</button>
    <button type="button" class="btn ghost" disabled={servers.probing} onclick={() => choose("manual")} title={t("account.addr.testHint")}>
      <RefreshCw size={14} class={servers.probing ? "spin" : ""} />{t("account.addr.test")}
    </button>
  </form>
  {#if err}<div class="err">{err}</div>{/if}
  <span class="hint">{t("account.addr.hint")}</span>
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
