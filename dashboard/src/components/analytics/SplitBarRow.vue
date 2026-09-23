<script setup>
import { computed } from 'vue'

const props = defineProps({
  label: { type: String, required: true },
  personal: { type: Number, default: 0 },
  workspace: { type: Number, default: 0 },
  org: { type: Number, default: 0 },
  max: { type: Number, required: true },
})
const pct = (v) => (props.max ? (v / props.max) * 100 : 0)
const total = computed(() => props.personal + props.workspace + props.org)
</script>

<template>
  <div class="grid items-center gap-3" style="grid-template-columns: 118px 1fr 26px">
    <div class="font-mono text-right overflow-hidden text-ellipsis whitespace-nowrap" :title="label"
      style="font-size:11.5px; color:var(--hm-text-secondary)">{{ label }}</div>
    <div class="flex overflow-hidden" style="height:14px; border-radius:2px; background:color-mix(in srgb, var(--hm-border-default) 45%, transparent)">
      <div v-if="personal > 0" style="height:100%; background:var(--hm-personal); transition:width .3s ease-out" :style="{ width: pct(personal) + '%' }"></div>
      <div v-if="workspace > 0" style="height:100%; background:var(--hm-workspace); transition:width .3s ease-out" :style="{ width: pct(workspace) + '%' }"></div>
      <div v-if="org > 0" style="height:100%; background:var(--hm-org); transition:width .3s ease-out" :style="{ width: pct(org) + '%' }"></div>
    </div>
    <div class="font-mono text-right" style="font-size:11.5px; color:var(--hm-text-primary); font-variant-numeric:tabular-nums">{{ total }}</div>
  </div>
</template>
