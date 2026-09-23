<script setup>
defineProps({
  label: { type: String, required: true },
  value: { type: [Number, String], required: true },
  sub: { type: String, required: true },
  trend: { type: Number, default: null },
})
</script>

<template>
  <div class="rounded-lg p-4 flex flex-col gap-1"
    style="background:var(--hm-bg-elevated); border:0.5px solid var(--hm-border-default)">
    <div class="font-mono uppercase" style="font-size:10px; letter-spacing:0.14em; color:var(--hm-text-tertiary)">{{ label }}</div>
    <div class="font-semibold" style="font-size:34px; line-height:1.05; letter-spacing:-0.04em; color:var(--hm-text-primary); font-variant-numeric:tabular-nums">{{ value }}</div>
    <div class="flex items-center gap-2" style="font-size:11.5px; color:var(--hm-text-tertiary)">
      <span v-if="trend != null" class="inline-flex items-center gap-1 rounded-sm font-mono"
        style="font-size:11px; font-weight:500; padding:1px 5px 1px 3px"
        :style="trend >= 0
          ? 'background:var(--hm-workspace-bg); color:var(--hm-workspace)'
          : 'background:var(--hm-danger-bg); color:var(--hm-danger)'">
        <svg width="14" height="14" viewBox="0 0 16 16" fill="none" aria-hidden="true">
          <path v-if="trend >= 0" d="M2.5 11.5L6.5 7.5L9 10L13.5 5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" />
          <path v-else d="M2.5 5L6.5 9L9 6.5L13.5 11.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" />
        </svg>
        {{ trend >= 0 ? '+' : '' }}{{ trend }}
      </span>
      <span>{{ sub }}</span>
    </div>
  </div>
</template>
