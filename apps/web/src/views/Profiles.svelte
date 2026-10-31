<script lang="ts">
  // "Who's watching?" (TV, lib/profiles.svelte.ts): los perfiles guardados en el aparato, como en
  // Netflix. Con el control: flechas entre perfiles, centro entra (sin contraseña). "Add profile"
  // entra con otra cuenta una sola vez. "Manage" pasa a editar perfiles: elegir su avatar (con la
  // sesion de ese perfil) o quitarlo (cierra su sesion en el server). El foco arranca en el perfil
  // activo.
  import { Check, Pencil, Plus, Trash2, X } from "@lucide/svelte";
  import { tick } from "svelte";
  import Avatar from "../components/Avatar.svelte";
  import AvatarPicker from "../components/AvatarPicker.svelte";
  import Logo from "../components/Logo.svelte";
  import { getToken } from "../lib/api";
  import { ask } from "../lib/confirm.svelte";
  import { rise } from "../lib/motion";
  import { profileBase, profiles, removeProfile, switchTo, type Profile } from "../lib/profiles.svelte";
  import { focusEl } from "../lib/spatial.svelte";
  import { session } from "../lib/session.svelte";

  let managing = $state(false);
  /** El perfil que se esta editando (avatar o quitar). */
  let editing = $state<Profile | null>(null);
  let root = $state<HTMLElement>();
  let panel = $state<HTMLElement>();
  const active = getToken();

  $effect(() => {
    if (!root || editing) return;
    tick().then(() => {
      const el = root?.querySelector<HTMLElement>(".pf.cur") ?? root?.querySelector<HTMLElement>(".pf");
      if (el) focusEl(el);
    });
  });

  // Al abrir el panel de un perfil, el foco va a su avatar elegido (la galeria llega del server:
  // se espera un poco) o, sin galeria, a "Done".
  $effect(() => {
    if (!panel) return;
    const el = panel;
    let n = 0;
    const id = setInterval(() => {
      const pick = el.querySelector<HTMLElement>("[aria-checked='true']");
      const done = el.querySelector<HTMLElement>(".btn.primary");
      const ready = !el.querySelector(".skel");
      if (pick || (ready && n > 2) || ++n > 20) {
        clearInterval(id);
        if (!el.contains(document.activeElement) || document.activeElement === done) {
          const b = pick ?? done;
          if (b) focusEl(b);
        }
      }
    }, 100);
    tick().then(() => {
      const b = el.querySelector<HTMLElement>(".btn.primary");
      if (b && !el.contains(document.activeElement)) focusEl(b);
    });
    return () => clearInterval(id);
  });

  function choose(p: Profile) {
    if (!managing) return switchTo(p);
    editing = p;
  }

  /** Esc/atras: con el panel abierto vuelve a la lista; administrando, deja de administrar; si
   *  no, con una sesion abierta, vuelve a la app (sin cambiar de perfil). */
  function onKey(e: KeyboardEvent) {
    if (e.key !== "Escape" || e.defaultPrevented || document.querySelector("[role=alertdialog]")) return;
    if (editing) editing = null;
    else if (managing) managing = false;
    else if (session.user) profiles.picking = false;
    else return;
    e.preventDefault();
  }

  async function remove(p: Profile) {
    const ok = await ask({
      title: `Remove ${p.username}?`,
      body: "This TV signs out of that account. You can add it again with its password.",
      confirm: "Remove",
      danger: true,
    });
    if (!ok) return;
    editing = null;
    await removeProfile(p);
    if (!profiles.list.length) {
      managing = false;
      add();
    }
  }

  function add() {
    profiles.picking = false;
    profiles.adding = true;
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="who" bind:this={root} data-nav-scope>
  {#if editing}
    {@const p = editing}
    <div class="edit card" bind:this={panel} data-nav-scope role="dialog" aria-label="Edit {p.username}" in:rise>
      <div class="head">
        <Avatar name={p.username} id={p.avatarId} size={56} to={{ base: profileBase(p), token: p.token }} />
        <h2 class="ellipsis">{p.username}</h2>
      </div>
      <div class="sub">Avatar</div>
      <AvatarPicker profile={p} />
      <div class="acts">
        <button class="btn danger" onclick={() => remove(p)}><Trash2 size={15} />Remove profile</button>
        <button class="btn primary" onclick={() => (editing = null)}><Check size={15} />Done</button>
      </div>
    </div>
  {/if}
  <div class="mark"><Logo size={40} /></div>
  <h1 in:rise>{managing ? "Manage profiles" : "Who's watching?"}</h1>
  <div class="row" in:rise={{ delay: 60 }}>
    {#each profiles.list as p (p.server + p.userId)}
      <button class="pf" class:cur={p.token === active} class:managing onclick={() => choose(p)} aria-label={managing ? `Edit ${p.username}` : p.username}>
        <span class="pic">
          <Avatar name={p.username} id={p.avatarId} size={112} to={{ base: profileBase(p), token: p.token }} />
          {#if managing}<span class="rm"><Pencil size={30} /></span>{/if}
        </span>
        <span class="name ellipsis">{p.username}</span>
      </button>
    {/each}
    {#if !managing}
      <button class="pf add" onclick={add} aria-label="Add profile">
        <span class="pic"><span class="plus"><Plus size={44} /></span></span>
        <span class="name">Add profile</span>
      </button>
    {/if}
  </div>
  <button class="btn ghost manage" onclick={() => (managing = !managing)}>
    {#if managing}<X size={15} />Done{:else}<Pencil size={15} />Manage profiles{/if}
  </button>
</div>

<style>
  .who {
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 26px;
    padding: 24px var(--gutter);
    text-align: center;
  }
  .mark {
    opacity: 0.9;
  }
  h1 {
    margin: 0;
    font-family: var(--display);
    font-size: 30px;
    font-weight: 700;
    letter-spacing: -0.02em;
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 26px;
    max-width: 820px;
  }
  .pf {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    width: 128px;
    padding: 8px 0;
    border-radius: var(--radius);
    color: var(--text-2);
  }
  .pf:focus-visible {
    outline: none;
    color: var(--text);
  }
  .pic {
    position: relative;
    display: grid;
    place-items: center;
    width: 112px;
    height: 112px;
    border-radius: 50%;
    transition: transform 0.2s cubic-bezier(0.2, 0.8, 0.2, 1);
  }
  .pf:hover .pic,
  .pf:focus-visible .pic {
    transform: scale(1.08);
    box-shadow:
      0 0 0 4px var(--accent),
      0 16px 34px -14px var(--media-scrim);
  }
  .plus {
    display: grid;
    place-items: center;
    width: 100%;
    height: 100%;
    border-radius: 50%;
    color: var(--text-2);
    background: var(--panel-2);
    border: var(--border) dashed var(--line-strong);
  }
  .rm {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    border-radius: 50%;
    color: var(--on-media);
    background: var(--media-scrim);
  }
  .name {
    max-width: 100%;
    font-size: 15px;
    font-weight: 600;
  }
  .cur .name {
    color: var(--text);
  }
  .manage {
    margin-top: 6px;
  }
  /* El panel de un perfil, encima de la lista. */
  .edit {
    position: fixed;
    z-index: 5;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    width: min(520px, calc(100% - 2 * var(--gutter)));
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 22px;
    text-align: left;
    box-shadow: var(--pop-shadow);
  }
  .head {
    display: flex;
    align-items: center;
    gap: 14px;
  }
  h2 {
    margin: 0;
    font-size: 20px;
  }
  .sub {
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-3);
  }
  .acts {
    display: flex;
    justify-content: space-between;
    gap: 10px;
    margin-top: 6px;
  }
</style>
