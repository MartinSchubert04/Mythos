<script lang="ts" module>
  export type ChartMetric = "requests" | "bytes" | "latency";
</script>

<script lang="ts">
  // Grafico de barras del trafico (SVG a mano, sin librerias). Una sola escala: la metrica la
  // elige el padre (requests con los errores apilados, datos enviados o latencia p95). Pasar el
  // mouse, tocar o las flechas del teclado muestran el detalle de cada barra.
  import { bytes } from "../lib/format";
  import { date, t as tr } from "../lib/i18n.svelte";
  import { compact, count, dur, niceMax, pct, type Bucket } from "../lib/traffic";

  let { data, hours, metric }: { data: Bucket[]; hours: number; metric: ChartMetric } = $props();

  let width = $state(0);
  let sel = $state<number | null>(null);
  let wrap: HTMLDivElement;

  const H = $derived(width && width < 520 ? 170 : 210);
  const PAD = { top: 10, right: 6, bottom: 24, left: 44 };

  const value = (b: Bucket) => (metric === "requests" ? b.requests : metric === "bytes" ? b.bytes_out : (b.p95_ms ?? 0));
  const fmtAxis = (v: number) => (metric === "requests" ? compact(v) : metric === "bytes" ? bytes(v).replace(" ", "") : dur(v).replace(" ", ""));

  const max = $derived(niceMax(Math.max(0, ...data.map(value))));
  const plotW = $derived(Math.max(0, width - PAD.left - PAD.right));
  const plotH = $derived(H - PAD.top - PAD.bottom);
  const slot = $derived(data.length ? plotW / data.length : 0);
  // Barras finas: 70 % del lugar, como mucho 22 px; al menos 1 px de aire entre una y otra.
  const barW = $derived(Math.max(1, Math.min(22, slot * 0.7, slot - 1)));
  const y = (v: number) => PAD.top + plotH - (v / max) * plotH;
  const empty = $derived(!data.some((b) => value(b) > 0));

  /** Rectangulo con las puntas de arriba redondeadas (el extremo del dato); la base queda recta. */
  function bar(x: number, top: number, bottom: number, w: number, round: boolean): string {
    const h = Math.max(0, bottom - top);
    if (h <= 0) return "";
    const r = round ? Math.min(4, w / 2, h) : 0;
    return `M${x},${bottom}V${top + r}${r ? `Q${x},${top} ${x + r},${top}` : ""}H${x + w - r}${r ? `Q${x + w},${top} ${x + w},${top + r}` : ""}V${bottom}Z`;
  }

  type Mark = { i: number; x: number; main: string; err: string };
  const marks = $derived.by((): Mark[] => {
    const base = PAD.top + plotH;
    return data.map((b, i) => {
      const x = PAD.left + i * slot + (slot - barW) / 2;
      if (metric !== "requests") return { i, x, main: bar(x, y(value(b)), base, barW, true), err: "" };
      // Errores (5xx) arriba de los que salieron bien, con 2 px del fondo entre los dos.
      const okTop = y(b.requests - b.errors);
      const errTop = y(b.requests);
      const gap = b.errors && b.requests - b.errors ? Math.min(2, (okTop - errTop) / 2) : 0;
      return {
        i,
        x,
        main: bar(x, okTop, base, barW, !b.errors),
        err: b.errors ? bar(x, errTop, okTop - gap, barW, true) : "",
      };
    });
  });

  // ---- etiquetas del eje X

  const sameDay = (a: number) => new Date(a).getHours() === 0;
  // Fechas con Intl en el idioma de la interfaz (lib/i18n.svelte.ts).
  const HM: Intl.DateTimeFormatOptions = { hour: "2-digit", minute: "2-digit", hourCycle: "h23" };
  const WD: Intl.DateTimeFormatOptions = { weekday: "short" };
  const MD: Intl.DateTimeFormatOptions = { month: "short", day: "numeric" };
  const WMD: Intl.DateTimeFormatOptions = { weekday: "short", month: "short", day: "numeric" };
  function xLabel(b: Bucket): string {
    const d = new Date(b.start);
    if (hours < 24 && data.length <= 30) return date(d, HM);
    if (hours < 24) return `${date(d, WD)} ${d.getDate()}`;
    return date(d, MD);
  }
  const ticks = $derived.by(() => {
    const out: { x: number; text: string }[] = [];
    let last = -Infinity;
    const minGap = 58;
    data.forEach((b, i) => {
      // 24 h: horas multiplo de 3; 7 dias: las medianoches; por dia: cualquiera, con aire.
      const ok = hours < 24 && data.length <= 30 ? new Date(b.start).getHours() % 3 === 0 : hours < 24 ? sameDay(b.start) : true;
      const x = PAD.left + i * slot + slot / 2;
      if (ok && x - last >= minGap && x > PAD.left + 14 && x < PAD.left + plotW - 14) {
        out.push({ x, text: xLabel(b) });
        last = x;
      }
    });
    return out;
  });

  // ---- detalle de una barra

  function range(b: Bucket): string {
    const s = new Date(b.start);
    const day = date(s, WMD);
    if (hours >= 24) return day;
    const t = (ms: number) => date(ms, HM);
    return `${day} · ${t(b.start)}–${t(b.end)}`;
  }
  const cur = $derived(sel != null ? data[sel] : null);
  const tipX = $derived(sel != null ? PAD.left + sel * slot + slot / 2 : 0);
  const summary = $derived.by(() => {
    if (!cur) return "";
    return `${range(cur)}: ${tr("admin.chart.summary", { r: count(cur.requests), e: count(cur.errors), b: bytes(cur.bytes_out) || "0 B" })}${cur.p95_ms != null ? `, p95 ${dur(cur.p95_ms)}` : ""}`;
  });

  function pick(e: PointerEvent) {
    if (!slot) return;
    const r = wrap.getBoundingClientRect();
    const i = Math.floor((e.clientX - r.left - PAD.left) / slot);
    sel = i >= 0 && i < data.length ? i : null;
  }
  function onKey(e: KeyboardEvent) {
    if (!data.length) return;
    if (e.key === "ArrowRight" || e.key === "ArrowLeft") {
      e.preventDefault();
      const d = e.key === "ArrowRight" ? 1 : -1;
      sel = sel == null ? (d > 0 ? 0 : data.length - 1) : Math.max(0, Math.min(data.length - 1, sel + d));
    } else if (e.key === "Home") sel = 0;
    else if (e.key === "End") sel = data.length - 1;
    else if (e.key === "Escape") sel = null;
  }

  const unit = $derived(hours >= 24 ? tr("admin.tr.perDay") : hours === 1 ? tr("admin.tr.perHour") : tr("admin.tr.perHours", { n: hours }));
  const label = $derived(
    `${metric === "requests" ? tr("admin.tr.requests") : metric === "bytes" ? tr("admin.tr.dataSent") : tr("admin.tr.p95")} ${unit}`,
  );
</script>

<!-- Para el teclado es un "slider" sobre las barras: las flechas recorren y se lee cada una. -->
<div
  class="chart"
  bind:this={wrap}
  bind:clientWidth={width}
  tabindex="0"
  role="slider"
  aria-label={tr("admin.chart.aria", { label })}
  aria-valuemin={0}
  aria-valuemax={Math.max(0, data.length - 1)}
  aria-valuenow={sel ?? 0}
  aria-valuetext={summary || tr("admin.chart.noBar")}
  onpointermove={pick}
  onpointerdown={pick}
  onpointerleave={(e) => e.pointerType === "mouse" && (sel = null)}
  onkeydown={onKey}
  onblur={() => (sel = null)}
>
  {#if width}
    <svg width={width} height={H} viewBox="0 0 {width} {H}" aria-hidden="true">
      <!-- Lineas guia: 0, la mitad y el tope -->
      {#each [0, 0.5, 1] as f (f)}
        {@const gy = y(max * f)}
        <line class="grid" class:base={f === 0} x1={PAD.left} x2={width - PAD.right} y1={gy} y2={gy} />
        <text class="axis" x={PAD.left - 8} y={gy} dy="0.35em" text-anchor="end">{fmtAxis(max * f)}</text>
      {/each}
      {#if cur}
        <rect class="band" x={PAD.left + (sel ?? 0) * slot} y={PAD.top} width={slot} height={plotH} />
      {/if}
      {#each marks as m (m.i)}
        <g class:dim={sel != null && sel !== m.i}>
          {#if m.main}<path class="m-main" d={m.main} />{/if}
          {#if m.err}<path class="m-err" d={m.err} />{/if}
        </g>
      {/each}
      {#each ticks as t (t.x)}
        <text class="axis" x={t.x} y={H - 6} text-anchor="middle">{t.text}</text>
      {/each}
    </svg>
    {#if empty}
      <div class="none">{tr("admin.chart.empty")}</div>
    {/if}
    {#if cur}
      <div class="tip" style="left: {Math.min(Math.max(tipX, 90), width - 90)}px">
        <div class="tip-t">{range(cur)}</div>
        <div class="tip-r"><span class="sw" class:main={metric === "requests"}></span>{tr("admin.tr.requests")}<b>{count(cur.requests)}</b></div>
        <div class="tip-r"><span class="sw" class:err={metric === "requests"}></span>{tr("admin.tr.serverErrors")}<b>{count(cur.errors)}{cur.errors ? ` · ${pct(cur.errors, cur.requests)}` : ""}</b></div>
        <div class="tip-r"><span class="sw" class:main={metric === "bytes"}></span>{tr("admin.tr.dataSent")}<b>{bytes(cur.bytes_out) || "0 B"}</b></div>
        <div class="tip-r"><span class="sw" class:main={metric === "latency"}></span>{tr("admin.tr.p95")}<b>{dur(cur.p95_ms)}</b></div>
      </div>
    {/if}
  {/if}
</div>

<!-- Lo mismo como tabla, para lectores de pantalla. Va dentro de un div de 1 px: una tabla no
     respeta width: 1px y, aunque recortada, ensanchaba la pagina (en el celular se iba de costado). -->
<div class="sr">
<table>
  <caption>{label}</caption>
  <thead><tr><th>{tr("admin.logs.time")}</th><th>{tr("admin.tr.requests")}</th><th>{tr("admin.tr.serverErrors")}</th><th>{tr("admin.tr.dataSent")}</th><th>p95</th></tr></thead>
  <tbody>
    {#each data as b (b.start)}
      {#if b.requests}
        <tr><td>{range(b)}</td><td>{b.requests}</td><td>{b.errors}</td><td>{bytes(b.bytes_out)}</td><td>{dur(b.p95_ms)}</td></tr>
      {/if}
    {/each}
  </tbody>
</table>
</div>

<style>
  .chart {
    position: relative;
    min-height: 170px;
    touch-action: pan-y;
    border-radius: var(--radius-sm);
  }
  svg {
    display: block;
    overflow: visible;
  }
  .grid {
    stroke: color-mix(in srgb, var(--line) 70%, transparent);
    stroke-width: 1;
    stroke-dasharray: 2 4;
  }
  .grid.base {
    stroke: var(--line-strong);
    stroke-dasharray: none;
  }
  .axis {
    fill: var(--text-3);
    font-family: var(--mono);
    font-size: 10.5px;
    font-variant-numeric: tabular-nums;
  }
  .band {
    fill: var(--hover);
  }
  .m-main {
    fill: var(--accent);
  }
  .m-err {
    fill: var(--danger);
  }
  g {
    transition: opacity 0.15s;
  }
  g.dim {
    opacity: 0.45;
  }
  .none {
    position: absolute;
    inset: 0 0 24px 44px;
    display: grid;
    place-items: center;
    color: var(--text-3);
    font-size: 12.5px;
    pointer-events: none;
  }
  .tip {
    position: absolute;
    top: 4px;
    transform: translateX(-50%);
    min-width: 172px;
    padding: 8px 10px;
    border: var(--border) solid var(--line-strong);
    border-radius: var(--radius-sm);
    background: var(--sidebar);
    box-shadow: var(--pop-shadow);
    font-size: 12px;
    pointer-events: none;
    z-index: 2;
  }
  .tip-t {
    font-weight: 600;
    margin-bottom: 4px;
    color: var(--text);
  }
  .tip-r {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--text-2);
    line-height: 1.6;
  }
  .tip-r b {
    margin-left: auto;
    padding-left: 12px;
    color: var(--text);
    font-family: var(--mono);
    font-weight: 600;
    font-size: 11.5px;
  }
  .sw {
    width: 8px;
    height: 8px;
    border-radius: 2px;
    flex: none;
  }
  .sw.main {
    background: var(--accent);
  }
  .sw.err {
    background: var(--danger);
  }
  .sr {
    position: absolute;
    left: 0;
    top: 0;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
