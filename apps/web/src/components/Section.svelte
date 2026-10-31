<script lang="ts" module>
  // Secciones de Settings que se abren y cierran. Cuales quedan abiertas se recuerda.
  const KEY = "mythos.settings.open";
  function load(): string[] {
    try {
      return JSON.parse(localStorage.getItem(KEY) ?? '["appearance"]');
    } catch {
      return ["appearance"];
    }
  }
  const opened = $state({ ids: load() });
  function persist() {
    try {
      localStorage.setItem(KEY, JSON.stringify(opened.ids));
    } catch {}
  }
</script>

<script lang="ts">
  import { ChevronDown } from "@lucide/svelte";
  import type { Component, Snippet } from "svelte";
  import { expand } from "../lib/motion";

  let {
    id,
    title,
    summary = "",
    icon: Icon,
    children,
  }: { id: string; title: string; summary?: string; icon: Component<{ size?: number }>; children: Snippet } = $props();

  const open = $derived(opened.ids.includes(id));

  function toggle() {
    opened.ids = open ? opened.ids.filter((x) => x !== id) : [...opened.ids, id];
    persist();
  }
</script>

<section class="sec card" class:open>
  <button class="head" type="button" aria-expanded={open} onclick={toggle}>
    <span class="ico"><Icon size={16} /></span>
    <span class="txt">
      <span class="title">{title}</span>
      {#if summary}<span class="sum ellipsis">{summary}</span>{/if}
    </span>
    <span class="chev"><ChevronDown size={16} /></span>
  </button>
  {#if open}
    <div class="body" transition:expand>
      <div class="inner">{@render children()}</div>
    </div>
  {/if}
</section>

<style>
  .sec {
    overflow: hidden;
    transition: border-color 0.2s;
  }
  .head {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 14px 16px;
    text-align: left;
    transition: background 0.15s;
  }
  .head:hover {
    background: var(--hover);
  }
  .ico {
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    flex: none;
    border-radius: var(--radius-sm);
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 14%, transparent);
  }
  .txt {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .title {
    font-weight: 620;
  }
  .sum {
    font-size: 12px;
    color: var(--text-3);
  }
  .chev {
    color: var(--text-3);
    display: grid;
    transition: transform 0.25s cubic-bezier(0.2, 0.8, 0.2, 1);
  }
  .open .chev {
    transform: rotate(180deg);
  }
  .inner {
    padding: 4px 16px 18px;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  :global([data-theme="athanor"]) .chev {
    transition-timing-function: steps(3);
  }
</style>
