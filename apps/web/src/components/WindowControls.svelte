<script lang="ts">
  // Minimizar, maximizar y cerrar (solo desktop: la ventana no tiene marco de Windows).
  import { Copy, Minus, Square, X } from "@lucide/svelte";
  import { onMount } from "svelte";
  import { t } from "../lib/i18n.svelte";
  import { windowCtl } from "../lib/platform";

  let { onmedia = false }: { onmedia?: boolean } = $props();
  let maximized = $state(false);

  onMount(() => {
    let un: (() => void) | undefined;
    let dead = false;
    const sync = async () => (maximized = await windowCtl.isMaximized());
    sync();
    windowCtl.onResized(sync).then((f) => (dead ? f() : (un = f)));
    return () => {
      dead = true;
      un?.();
    };
  });
</script>

<div class="controls" class:onmedia>
  <button class="win" title={t("browse.win.minimize")} onclick={() => windowCtl.minimize()}><Minus size={15} /></button>
  <button class="win" title={maximized ? t("browse.win.restore") : t("browse.win.maximize")} onclick={() => windowCtl.toggleMaximize()}>
    {#if maximized}<Copy size={12} />{:else}<Square size={12} />{/if}
  </button>
  <button class="win close" title={t("common.close")} onclick={() => windowCtl.close()}><X size={16} /></button>
</div>

<style>
  .controls {
    display: flex;
    align-self: stretch;
  }
  .win {
    width: 46px;
    display: grid;
    place-items: center;
    color: var(--text-2);
    transition:
      background 0.12s,
      color 0.12s;
  }
  .win:hover {
    background: var(--hover);
    color: var(--text);
  }
  .win.close:hover {
    background: var(--win-close);
    color: var(--win-close-ink);
  }
  .onmedia .win {
    color: var(--on-media-2);
  }
  .onmedia .win:hover {
    color: var(--on-media);
    background: var(--media-line);
  }
</style>
