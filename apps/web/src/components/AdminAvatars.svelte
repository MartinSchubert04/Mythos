<script lang="ts">
  // Admin → Avatars: la galeria de avatares que cada usuario elige en Settings → Account (y en la
  // TV al sumar un perfil). Se suben varias de una (PNG, JPEG, WebP o GIF, hasta 2 MB cada una; el
  // server mira el contenido, no el nombre) y se borran de a una: quien la tenia vuelve a la inicial.
  import { ImagePlus, LoaderCircle, Trash2 } from "@lucide/svelte";
  import { api, avatarUrl, errText } from "../lib/api";
  import { ask } from "../lib/confirm.svelte";
  import { session } from "../lib/session.svelte";
  import { toast } from "../lib/toast.svelte";
  import type { Avatar } from "../lib/types";

  const MAX = 2 * 1024 * 1024;

  let list = $state<Avatar[] | null>(null);
  let error = $state("");
  let uploading = $state(0);
  let input = $state<HTMLInputElement>();

  function load() {
    api
      .avatars()
      .then((l) => ((list = l), (error = "")))
      .catch((e) => (error = errText(e)));
  }
  load();

  async function upload(files: FileList | null) {
    if (!files?.length) return;
    const all = [...files];
    uploading = all.length;
    let ok = 0;
    for (const f of all) {
      try {
        if (f.size > MAX) throw new Error("Images can be up to 2 MB");
        const a = await api.uploadAvatar(f, f.name.replace(/\.[^.]+$/, ""));
        list = [...(list ?? []), a];
        ok++;
      } catch (e) {
        toast(`${f.name}: ${errText(e)}`, true);
      } finally {
        uploading--;
      }
    }
    if (ok) toast(ok === 1 ? "Avatar added" : `${ok} avatars added`);
    if (input) input.value = "";
  }

  async function remove(a: Avatar) {
    const ok = await ask({
      title: `Delete ${a.name}?`,
      body: "Anyone using it goes back to their initial.",
      confirm: "Delete",
      danger: true,
    });
    if (!ok) return;
    try {
      await api.deleteAvatar(a.id);
      list = list?.filter((x) => x.id !== a.id) ?? null;
      if (session.user?.avatar_id === a.id) session.user = { ...session.user, avatar_id: null };
    } catch (e) {
      toast(errText(e), true);
    }
  }
</script>

<div class="head">
  <span class="faint">Everyone picks theirs in Settings → Account. PNG, JPEG, WebP or GIF, up to 2 MB each; square images look best.</span>
  <div class="grow"></div>
  <label class="btn primary" class:busy={uploading > 0}>
    {#if uploading}<LoaderCircle size={15} class="spin" />Uploading {uploading}…{:else}<ImagePlus size={15} />Add images{/if}
    <input bind:this={input} type="file" accept="image/png,image/jpeg,image/webp,image/gif" multiple disabled={uploading > 0} onchange={(e) => upload(e.currentTarget.files)} />
  </label>
</div>

{#if error && !list}
  <div class="empty"><h3>Couldn't load the avatars</h3><span>{error}</span></div>
{:else if !list}
  <div class="grid">{#each Array(6) as _, i (i)}<span class="skel cell"></span>{/each}</div>
{:else if !list.length}
  <div class="empty"><ImagePlus size={28} /><h3>No avatars yet</h3><span>Add a few images and everyone can pick one.</span></div>
{:else}
  <div class="grid">
    {#each list as a (a.id)}
      <div class="item">
        <img class="cell" src={avatarUrl(a.id)} alt={a.name} loading="lazy" draggable="false" />
        <span class="name ellipsis" title={a.name}>{a.name}</span>
        <button class="icon-btn del" title="Delete {a.name}" aria-label="Delete {a.name}" onclick={() => remove(a)}><Trash2 size={15} /></button>
      </div>
    {/each}
  </div>
{/if}

<style>
  .head {
    display: flex;
    align-items: center;
    gap: 14px;
    margin-bottom: 18px;
    flex-wrap: wrap;
  }
  .grow {
    flex: 1;
  }
  label.btn {
    position: relative;
    cursor: pointer;
  }
  label.btn input {
    position: absolute;
    width: 1px;
    height: 1px;
    opacity: 0;
    pointer-events: none;
  }
  label.busy {
    opacity: 0.7;
    pointer-events: none;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(112px, 1fr));
    gap: 16px;
  }
  .item {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 12px 8px 10px;
    border-radius: var(--radius);
    background: var(--panel);
    border: var(--border) solid var(--line);
  }
  .cell {
    width: 80px;
    height: 80px;
    border-radius: 50%;
    object-fit: cover;
    background: var(--panel-2);
  }
  .skel.cell {
    justify-self: center;
  }
  .name {
    max-width: 100%;
    font-size: 12.5px;
    color: var(--text-2);
  }
  .del {
    position: absolute;
    top: 6px;
    right: 6px;
  }
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 40px 0;
    color: var(--text-3);
    text-align: center;
  }
  .empty h3 {
    margin: 4px 0 0;
    color: var(--text);
  }
</style>
