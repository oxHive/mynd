<script setup>
// SVG growth chart: cumulative area+line over per-period columns.
// Width is measured from the parent — never let the SVG's own width drive
// layout (that caused horizontal-overflow in the design prototype).
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { DAY } from '../../lib/analytics.js'

const props = defineProps({
  buckets: { type: Array, required: true },
  step: { type: Number, required: true },
})

const wrap = ref(null)
const w = ref(320) // start small, grow to fit
let ro
function measure() {
  const el = wrap.value; if (!el) return
  const cap = el.parentElement ? el.parentElement.clientWidth - 2 : Infinity
  const cw = Math.min(el.clientWidth || cap, cap)
  if (cw > 0) w.value = cw
}
onMounted(() => {
  ro = new ResizeObserver(measure)
  if (wrap.value) { ro.observe(wrap.value); if (wrap.value.parentElement) ro.observe(wrap.value.parentElement) }
  measure()
})
onBeforeUnmount(() => ro?.disconnect())

const H = 208, padL = 36, padR = 44, padT = 16, padB = 28
const ih = H - padT - padB
const iw = computed(() => Math.max(120, w.value - padL - padR))
const n = computed(() => props.buckets.length || 1)
const band = computed(() => iw.value / n.value)
const maxNew = computed(() => Math.max(1, ...props.buckets.map((b) => b.count)))
const maxCum = computed(() => Math.max(1, ...props.buckets.map((b) => b.cum)))
const yNew = (v) => padT + ih - (v / maxNew.value) * ih * 0.66 // columns top out at 66% height
const yCum = (v) => padT + ih - (v / maxCum.value) * ih
const cx = (i) => padL + band.value * (i + 0.5)

const linePath = computed(() => props.buckets.map((b, i) => `${i ? 'L' : 'M'}${cx(i).toFixed(1)},${yCum(b.cum).toFixed(1)}`).join(' '))
const areaPath = computed(() => props.buckets.length
  ? `${linePath.value} L${cx(n.value - 1).toFixed(1)},${padT + ih} L${cx(0).toFixed(1)},${padT + ih} Z` : '')
const tickEvery = computed(() => Math.max(1, Math.ceil(n.value / 7)))
const barW = computed(() => Math.max(3, Math.min(22, band.value - (props.step === 1 ? 3 : 6))))
const gridFractions = [0, 0.25, 0.5, 0.75, 1]

const hover = ref(null)
function onMove(e) {
  const r = e.currentTarget.getBoundingClientRect()
  const i = Math.floor((e.clientX - r.left - padL) / band.value)
  hover.value = i >= 0 && i < n.value ? i : null
}
const hb = computed(() => (hover.value != null ? props.buckets[hover.value] : null))

const fmtMonth = (ts) => new Date(ts * 1000).toLocaleDateString('en-US', { month: 'short' })
const fmtDay = (ts) => new Date(ts * 1000).toLocaleDateString('en-US', { month: 'short', day: 'numeric' })
const tick = (b) => (props.step === 30 ? fmtMonth(b.s) : fmtDay(b.s))
const tipDate = computed(() => !hb.value ? '' :
  tick(hb.value) + (props.step === 7 ? ' → ' + fmtDay(hb.value.e - DAY) : ''))
const tipLeft = computed(() => hover.value == null ? 0 : Math.min(Math.max(cx(hover.value), 70), w.value - 70))
</script>

<template>
  <div ref="wrap" class="growth-wrap">
    <svg :width="w" :height="H" style="display:block; overflow:visible" @mousemove="onMove" @mouseleave="hover = null">
      <g v-for="f in gridFractions" :key="f">
        <line :x1="padL" :x2="padL + iw" :y1="yCum(maxCum * (1 - f))" :y2="yCum(maxCum * (1 - f))" stroke="var(--hm-border-default)" stroke-width="1" />
        <text :x="padL - 8" :y="yCum(maxCum * (1 - f)) + 3.5" class="font-mono" style="font-size:9.5px" fill="var(--hm-text-tertiary)" text-anchor="end">{{ Math.round(maxCum * (1 - f)) }}</text>
      </g>
      <path v-if="areaPath" :d="areaPath" fill="color-mix(in srgb, var(--hm-org) 12%, transparent)" />
      <rect v-for="(b, i) in buckets" :key="'b' + i"
        :x="cx(i) - barW / 2" :y="yNew(b.count)" :width="barW"
        :height="Math.max(b.count ? 1.5 : 0, padT + ih - yNew(b.count))" rx="1"
        fill="var(--hm-accent)" :style="{ opacity: hover === i ? 1 : 0.78, transition: 'opacity .12s' }" />
      <path v-if="areaPath" :d="linePath" fill="none" stroke="var(--hm-org)" stroke-width="1.75" stroke-linejoin="round" />
      <template v-if="hb && hover != null">
        <line :x1="cx(hover)" :x2="cx(hover)" :y1="padT" :y2="padT + ih" stroke="var(--hm-text-tertiary)" stroke-width="1" stroke-dasharray="3 3" />
        <circle :cx="cx(hover)" :cy="yCum(hb.cum)" r="3.5" fill="var(--hm-org)" stroke="var(--hm-bg-elevated)" stroke-width="2" />
      </template>
      <template v-for="(b, i) in buckets" :key="'t' + i">
        <text v-if="i % tickEvery === 0" :x="cx(i)" :y="H - 9" class="font-mono" style="font-size:9.5px" fill="var(--hm-text-tertiary)" text-anchor="middle">{{ tick(b) }}</text>
      </template>
    </svg>
    <div v-if="hb" class="growth-tip" :style="{ left: tipLeft + 'px' }">
      <div class="font-mono" style="font-size:10px; color:#7e8a8e; margin-bottom:4px; white-space:nowrap">{{ tipDate }}</div>
      <div class="growth-tip-row"><span class="sw" style="background:var(--hm-org)"></span>{{ hb.cum }} total</div>
      <div class="growth-tip-row"><span class="sw" style="background:var(--hm-accent)"></span>{{ hb.count }} added</div>
    </div>
  </div>
</template>

<style scoped>
.growth-wrap { position: relative; min-width: 0; overflow: hidden; }
.growth-tip {
  position: absolute; top: 4px; transform: translateX(-50%);
  background: #0b0f10; border: 1px solid #20292c; border-radius: 4px;
  padding: 7px 10px; pointer-events: none; z-index: 3;
  box-shadow: 0 10px 15px -3px rgba(11,15,16,.10), 0 4px 6px -2px rgba(11,15,16,.05);
}
.growth-tip-row { display: flex; align-items: center; gap: 6px; font-size: 11.5px; color: #fff; white-space: nowrap; }
.sw { width: 9px; height: 9px; border-radius: 2px; flex-shrink: 0; display: inline-block; }
</style>
