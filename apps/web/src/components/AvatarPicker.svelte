<script lang="ts">
  // Elegir el avatar propio de la galeria del server (Settings → Account). Una grilla de botones:
  // con el control de la TV se recorre con las flechas. "None" vuelve a la inicial.
  // Con `profile` ("Who's watching?" en la TV): el de ese perfil guardado, con su server y su
  // sesion (sin entrar a esa cuenta).
  import { Check, ImageOff } from "@lucide/svelte";
  import { api, avatarUrl, errText, getToken } from "../lib/api";
  import { syncProfile, updateProfile, type Profile } from "../lib/profiles.svelte";
  import { session } from "../lib/session.svelte";
  import { toast } from "../lib/toast.svelte";
  import type { Avatar } from "../lib/types";

  let { onpicked, profile }: { onpicked?: () => void; profile?: Profile } = $props();

  let list = $state<Avatar[] | null>(null);
  let error = $state("");
  let busy = $state(false);
  const to = $derived(profile ? { base: profile.server, token: profile.token } : undefined);

  // Una vez al montar (el padre lo vuelve a montar con otro perfil).
  function load() {
    api
      .avatars(to)
      .then((l) => (list = l))
      .catch((e) => {
        list = [];
        error = errText(e);
      });
  }
  load();

  const current = $derived(profile ? profile.avatarId : (session.user?.avatar_id ?? null));

  async function pick(id: number | null) {
    if (busy || id === current) return onpicked?.();
    busy = true;
    try {
      const u = await api.setAvatar(id, to);
      if (profile) updateProfile(profile, u);
      // El perfil (o la cuenta) de la sesion activa: tambien la barra lateral.
      if (!profile || (profile.token === getToken() && session.user?.id === u.id)) {
        session.user = u;
        syncProfile(u);
      }
      onpicked?.();
    } catch (e) {
      toast(errText(e), true);
    } finally {
      busy = false;
    }
  }
</script>

{#if !list}
  <div class="grid">{#each Array(4) as _, i (i)}<span class="skel cell"></span>{/each}</div>
{:else if !list.length}
  <span class="faint small">{error ? `Couldn't load the avatars: ${error}` : "No avatars yet. An admin can add some in Admin → Avatars."}</span>
{:else}
  <div class="grid" role="radiogroup" aria-label="Avatar">
    <button class="cell none" class:on={current === null} role="radio" aria-checked={current === null} title="No avatar" aria-busy={busy} onclick={() => pick(null)}>
      <ImageOff size={20} />
      {#if current === null}<span class="ck"><Check size={12} /></span>{/if}
    </button>
    {#each list as a (a.id)}
      <button class="cell" class:on={current === a.id} role="radio" aria-checked={current === a.id} title={a.name} aria-busy={busy} onclick={() => pick(a.id)}>
        <img src={avatarUrl(a.id, to)} alt={a.name} draggable="false" loading="lazy" />
        {#if current === a.id}<span class="ck"><Check size={12} /></span>{/if}
      </button>
    {/each}
  </div>
{/if}

<style>
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, 64px);
    gap: 12px;
  }
  .cell {
    position: relative;
    width: 64px;
    height: 64px;
    border-radius: 50%;
    padding: 0;
    overflow: visible;
    display: grid;
    place-items: center;
    background: var(--panel-2);
    border: var(--border) solid var(--line);
    color: var(--text-3);
    transition: transform 0.18s cubic-bezier(0.2, 0.8, 0.2, 1);
  }
  .cell img {
    width: 100%;
    height: 100%;
    border-radius: 50%;
    object-fit: cover;
  }
  .cell:hover,
  .cell:focus-visible {
    transform: scale(1.06);
  }
  .cell.on {
    border-color: var(--accent);
    box-shadow: 0 0 0 2px var(--accent);
  }
  .ck {
    position: absolute;
    right: -2px;
    bottom: -2px;
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    border-radius: 50%;
    color: var(--accent-ink);
    background: var(--accent);
  }
  .skel.cell {
    border: 0;
  }
  .small {
    font-size: 12.5px;
  }
</style>
