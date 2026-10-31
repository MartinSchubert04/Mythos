<script lang="ts">
  // La marca de Mythos: una M de dos cumbres con una estrella, en el acento del tema.
  // Con `shine`, un brillo la cruza cada tanto (login, splash).
  let { size = 28, shine = false, word = false }: { size?: number; shine?: boolean; word?: boolean } = $props();
  const uid = Math.random().toString(36).slice(2, 8);
</script>

<span class="logo" class:shine style="--s: {size}px">
  <svg viewBox="0 0 512 512" width={size} height={size} aria-hidden="true">
    <defs>
      <linearGradient id="lg-{uid}" x1="0" y1="0" x2="1" y2="1">
        <stop offset="0" style="stop-color: var(--accent)" />
        <stop offset="1" style="stop-color: color-mix(in srgb, var(--accent) 55%, var(--text))" />
      </linearGradient>
      <clipPath id="lc-{uid}">
        <path d="M120 372 V176 L256 316 L392 176 V372" />
      </clipPath>
    </defs>
    <path
      class="m"
      d="M120 372 V176 L256 316 L392 176 V372"
      fill="none"
      stroke="url(#lg-{uid})"
      stroke-width="58"
      stroke-linecap="round"
      stroke-linejoin="round"
    />
    <path class="star" d="M256 92 L268 126 L302 138 L268 150 L256 184 L244 150 L210 138 L244 126 Z" />
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
  .star {
    fill: var(--text);
    transform-origin: 256px 138px;
  }
  .word {
    font-family: var(--display);
    font-weight: 700;
    font-size: calc(var(--s) * 0.62);
    letter-spacing: -0.02em;
    color: var(--text);
  }
  /* Brillo: la estrella titila y un reflejo cruza la marca. */
  .shine .star {
    animation: twinkle 3.2s ease-in-out infinite;
  }
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
    filter: drop-shadow(0 0 calc(var(--s) * 0.18) color-mix(in srgb, var(--accent) 45%, transparent));
  }
  @keyframes twinkle {
    0%,
    70%,
    100% {
      transform: scale(1) rotate(0);
      opacity: 1;
    }
    80% {
      transform: scale(1.35) rotate(45deg);
      opacity: 0.85;
    }
  }
</style>
