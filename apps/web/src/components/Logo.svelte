<script lang="ts">
  // La marca de Mythos: una M sólida en 3D vista de tres cuartos (frente, cantos de arriba y
  // costados). Los tonos salen del tema: el frente es el color del texto y las caras de la
  // extrusión lo mezclan con el fondo, así en un tema oscuro queda clara con costados oscuros
  // (como el ícono de la app, en los grises de Neovim) y en uno claro se invierte sola.
  // Con `shine`, un reflejo la cruza cada tanto (login, splash).
  let { size = 28, shine = false, word = false }: { size?: number; shine?: boolean; word?: boolean } = $props();
</script>

<span class="logo" class:shine style="--s: {size}px">
  <svg viewBox="0 0 512 512" width={size} height={size} aria-hidden="true">
    <g transform="translate(256 256) scale(1.7308) translate(-273 -247)">
      <path class="top" d="M319.6 143.6 L373 135 L407 113 L353.6 121.6 Z" />
      <path class="side" d="M373 135 L373 343 L407 321 L407 113 Z" />
      <path class="side" d="M319.6 224.8 L256 312.9 L290 290.9 L353.6 202.8 Z" />
      <path class="top" d="M192.4 164.4 L256 231.7 L290 209.7 L226.4 142.4 Z" />
      <path class="top" d="M139 173 L192.4 164.4 L226.4 142.4 L173 151 Z" />
      <path class="side" d="M192.4 245.6 L192.4 372.4 L226.4 350.4 L226.4 223.6 Z" />
      <path
        class="front"
        d="M139 381 L139 173 L192.4 164.4 L256 231.7 L319.6 143.6 L373 135 L373 343 L319.6 351.6 L319.6 224.8 L256 312.9 L192.4 245.6 L192.4 372.4 Z"
      />
    </g>
  </svg>
  {#if word}<span class="word">Mythos</span>{/if}
</span>

<style>
  .logo {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: calc(var(--s) * 0.32);
    flex: none;
  }
  svg {
    display: block;
    overflow: visible;
  }
  .front {
    fill: var(--text);
  }
  .top {
    fill: color-mix(in srgb, var(--text) 62%, var(--bg));
    stroke: color-mix(in srgb, var(--text) 62%, var(--bg));
    stroke-width: 0.6;
    stroke-linejoin: round;
  }
  .side {
    fill: color-mix(in srgb, var(--text) 36%, var(--bg));
    stroke: color-mix(in srgb, var(--text) 36%, var(--bg));
    stroke-width: 0.6;
    stroke-linejoin: round;
  }
  .word {
    font-family: var(--display);
    font-weight: 700;
    font-size: calc(var(--s) * 0.62);
    letter-spacing: -0.02em;
    color: var(--text);
  }
  /* Brillo: un reflejo cruza la marca cada tanto. */
  .shine::after {
    content: "";
    position: absolute;
    top: -10%;
    bottom: -10%;
    left: 0;
    width: calc(var(--s) * 0.5);
    background: linear-gradient(90deg, transparent, color-mix(in srgb, var(--text) 45%, transparent), transparent);
    mix-blend-mode: overlay;
    pointer-events: none;
    animation: glow-sweep 4.5s ease-in-out infinite;
  }
  .shine svg {
    filter: drop-shadow(0 0 calc(var(--s) * 0.16) color-mix(in srgb, var(--text) 22%, transparent));
  }
</style>
