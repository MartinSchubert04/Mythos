<script lang="ts">
  // Admin → Avatars: la galeria de avatares que cada usuario elige en Settings → Account (y en la
  // TV al sumar un perfil). El server trae unos de fabrica. Se suben varias de una (PNG, JPEG, WebP
  // o GIF, hasta 2 MB cada una; el server mira el contenido, no el nombre): cada una pasa por el
  // recorte (AvatarCropper: centrar y zoom) antes de subirse. Se borran de a una: quien la tenia
  // vuelve a la inicial.
  import { ImagePlus, LoaderCircle, Trash2 } from "@lucide/svelte";
  import { api, avatarUrl, errText } from "../lib/api";
  import { ask } from "../lib/confirm.svelte";
  import { t } from "../lib/i18n.svelte";
  import { session } from "../lib/session.svelte";
  import { toast } from "../lib/toast.svelte";
  import type { Avatar } from "../lib/types";
  import AvatarCropper from "./AvatarCropper.svelte";

  const MAX = 2 * 1024 * 1024;

  let list = $state<Avatar[] | null>(null);
  let error = $state("");
  let uploading = $state(0);
  let input = $state<HTMLInputElement>();
  /** La imagen que se esta recortando y las que siguen. */
  let crop = $state<{ files: File[]; i: number; ok: number } | null>(null);

  function load() {
    api
      .avatars()
      .then((l) => ((list = l), (error = "")))
      .catch((e) => (error = errText(e)));
  }
  load();

  function pick(files: FileList | null) {
    if (files?.length) crop = { files: [...files], i: 0, ok: 0 };
    if (input) input.value = "";
  }

  /** Lo que devolvio el recorte de la imagen actual: se sube y se pasa a la siguiente. */
  async function cropped(r: { blob: Blob; name: string } | null) {
    if (!crop || uploading) return;
    const c = crop;
    const f = c.files[c.i];
    if (r) {
      uploading++;
      try {
        if (r.blob.size > MAX) throw new Error(t("admin.av.tooBig"));
        const a = await api.uploadAvatar(r.blob, r.name);
        list = [...(list ?? []), a];
        c.ok++;
      } catch (e) {
        toast(`${f.name}: ${errText(e)}`, true);
      } finally {
        uploading--;
      }
    }
    if (c.i + 1 < c.files.length) crop = { ...c, i: c.i + 1 };
    else {
      crop = null;
      if (c.ok) toast(t("admin.av.added", { n: c.ok }));
    }
  }

  async function remove(a: Avatar) {
    const ok = await ask({
      title: t("admin.av.deleteTitle", { name: a.name }),
      body: t("admin.av.deleteBody"),
      confirm: t("common.delete"),
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
  <span class="faint">{t("admin.av.hint")}</span>
  <div class="grow"></div>
  <label class="btn primary" class:busy={uploading > 0 || !!crop}>
    {#if uploading}<LoaderCircle size={15} class="spin" />{t("admin.av.uploading")}{:else}<ImagePlus size={15} />{t("admin.av.add")}{/if}
    <input bind:this={input} type="file" accept="image/png,image/jpeg,image/webp,image/gif" multiple disabled={uploading > 0 || !!crop} onchange={(e) => pick(e.currentTarget.files)} />
  </label>
</div>

{#if crop}
  {#key crop.i}
    <AvatarCropper file={crop.files[crop.i]} index={crop.i + 1} total={crop.files.length} ondone={cropped} />
  {/key}
{/if}

{#if error && !list}
  <div class="empty"><h3>{t("admin.av.loadError")}</h3><span>{error}</span></div>
{:else if !list}
  <div class="grid">{#each Array(6) as _, i (i)}<span class="skel cell"></span>{/each}</div>
{:else if !list.length}
  <div class="empty"><ImagePlus size={28} /><h3>{t("admin.av.none")}</h3><span>{t("admin.av.noneHint")}</span></div>
{:else}
  <div class="grid">
    {#each list as a (a.id)}
      <div class="item">
        <img class="cell" src={avatarUrl(a.id)} alt={a.name} loading="lazy" draggable="false" />
        <span class="name ellipsis" title={a.name}>{a.name}</span>
        <button class="icon-btn del" title={t("admin.av.deleteTip", { name: a.name })} aria-label={t("admin.av.deleteTip", { name: a.name })} onclick={() => remove(a)}><Trash2 size={15} /></button>
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
