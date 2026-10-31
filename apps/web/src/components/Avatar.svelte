<script lang="ts">
  // El avatar de una cuenta: la imagen que eligio de la galeria (avatars.rs en el server) o, sin
  // avatar (o si la imagen no carga: modo local sin server, un server viejo), la inicial sobre el
  // acento, como siempre. `to`: el server y la sesion de otro perfil (selector de la TV).
  import { avatarUrl } from "../lib/api";

  let {
    name,
    id = null,
    size = 26,
    to,
  }: { name: string; id?: number | null; size?: number; to?: { base: string; token: string | null } } = $props();

  let failed = $state<number | null>(null);
  const src = $derived(id != null && failed !== id ? avatarUrl(id, to) : "");
</script>

<span class="avatar" style="--s: {size}px" aria-hidden="true">
  {#if src}
    <img {src} alt="" draggable="false" onerror={() => (failed = id)} />
  {:else}
    {name.slice(0, 1).toUpperCase()}
  {/if}
</span>

<style>
  .avatar {
    flex: none;
    display: grid;
    place-items: center;
    width: var(--s);
    height: var(--s);
    border-radius: 50%;
    overflow: hidden;
    font-weight: 700;
    font-size: calc(var(--s) * 0.46);
    line-height: 1;
    color: var(--accent-ink);
    background: var(--primary-bg);
  }
  img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
</style>
