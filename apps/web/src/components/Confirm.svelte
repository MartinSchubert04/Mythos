<script lang="ts">
  // Modal de confirmacion. Enter confirma, Esc cancela.
  import { TriangleAlert, CircleHelp } from "@lucide/svelte";
  import { confirmState as c } from "../lib/confirm.svelte";
  import { fadeQuick, pop } from "../lib/motion";

  let ok = $state<HTMLButtonElement>();
  let field = $state<HTMLInputElement>();
  $effect(() => {
    (field ?? ok)?.focus();
  });

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      c.resolve?.(false);
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="scrim" role="presentation" onclick={() => c.resolve?.(false)} transition:fadeQuick|global></div>
<div class="center">
  <div class="dialog card" class:danger={c.danger} role="alertdialog" aria-modal="true" transition:pop|global>
    <div class="icon">
      {#if c.danger}<TriangleAlert size={20} />{:else}<CircleHelp size={20} />{/if}
    </div>
    <div class="body">
      <h3>{c.title}</h3>
      {#if c.body}<p class="muted">{c.body}</p>{/if}
      {#if c.input}
        <input
          bind:this={field}
          class="field-in"
          type={c.input.secret ? "password" : "text"}
          placeholder={c.input.placeholder}
          bind:value={c.value}
          onkeydown={(e) => e.key === "Enter" && c.value && (e.preventDefault(), c.resolve?.(true))}
        />
      {/if}
    </div>
    <div class="row actions">
      <button class="btn" onclick={() => c.resolve?.(false)}>Cancel</button>
      <button bind:this={ok} class="btn {c.danger ? 'danger solid' : 'primary'}" disabled={!!c.input && !c.value} onclick={() => c.resolve?.(true)}>
        {c.confirm}
      </button>
    </div>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.45);
    backdrop-filter: blur(2px);
    z-index: 75;
  }
  .center {
    position: fixed;
    inset: 0;
    z-index: 76;
    display: grid;
    place-items: center;
    pointer-events: none;
  }
  .dialog {
    pointer-events: auto;
    width: min(440px, calc(100vw - 32px));
    padding: 20px;
    background: var(--sidebar);
    box-shadow: var(--pop-shadow);
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 6px 14px;
  }
  .icon {
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    border-radius: 50%;
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 14%, transparent);
  }
  .danger .icon {
    color: var(--danger);
    background: color-mix(in srgb, var(--danger) 14%, transparent);
  }
  h3 {
    margin: 6px 0 4px;
    font-size: 15px;
  }
  p {
    margin: 0;
    line-height: 1.5;
    font-size: 13px;
  }
  .field-in {
    margin-top: 12px;
    width: 100%;
  }
  .actions {
    grid-column: 1 / -1;
    justify-content: flex-end;
    margin-top: 14px;
  }
</style>
