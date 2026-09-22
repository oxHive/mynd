<script setup>
import { computed } from 'vue'
import LayerBadge from '../shared/LayerBadge.vue'

const props = defineProps({
  node: Object,
  x: Number,
  y: Number,
  connectionCount: { type: Number, default: 0 },
  // Bounds of the container the card must stay inside, e.g. the canvas wrap.
  bounds: { type: Object, default: () => ({ w: 0, h: 0 }) },
})

const CARD_W = 220
const CARD_H = 128

const snippet = computed(() => {
  const c = props.node?.content || ''
  return c.length > 90 ? c.slice(0, 90).trimEnd() + '…' : c
})

// Clamp near the cursor but keep the card fully inside the container —
// flips to the left/above when it would otherwise overflow the right/bottom edge.
const pos = computed(() => {
  const { w, h } = props.bounds
  let left = props.x + 14
  let top = props.y - 10
  if (w) {
    if (left + CARD_W > w - 8) left = props.x - 14 - CARD_W
    if (left < 8) left = 8
  }
  if (h) {
    if (top + CARD_H > h - 8) top = h - CARD_H - 8
    if (top < 8) top = 8
  }
  return { left, top }
})
</script>

<template>
  <div class="absolute pointer-events-none rounded-lg p-3"
    style="width:220px; background:var(--hm-bg-overlay); border:0.5px solid var(--hm-border-default); box-shadow:0 8px 24px rgba(0,0,0,0.3); z-index:10"
    :style="{ left: pos.left + 'px', top: pos.top + 'px' }">
    <div class="flex items-start justify-between gap-2 mb-1">
      <span style="font-size:12px; font-weight:500; color:var(--hm-text-primary)">{{ node.title }}</span>
      <LayerBadge :layer="node.layer" />
    </div>
    <div v-if="snippet" style="font-size:11px; line-height:1.5; color:var(--hm-text-secondary)">{{ snippet }}</div>
    <div class="flex items-center justify-between mt-2">
      <div class="flex flex-wrap gap-1">
        <span v-for="tag in (node.tags||[]).slice(0,3)" :key="tag"
          class="font-mono rounded-sm px-1 py-0.5"
          style="font-size:9px; background:var(--hm-bg-elevated); color:var(--hm-text-tertiary)">{{ tag }}</span>
      </div>
      <span class="font-mono shrink-0" style="font-size:10px; color:var(--hm-text-tertiary)">
        {{ connectionCount }} connection{{ connectionCount === 1 ? '' : 's' }}
      </span>
    </div>
  </div>
</template>
