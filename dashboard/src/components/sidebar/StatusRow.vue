<script setup>
defineProps({
  dot: String,      // 'green' | 'amber' | 'red' | 'gray'
  pulse: { type: Boolean, default: false },
  k: String,         // label, e.g. 'server'
  v: String,         // value, e.g. 'localhost:3456'
  text: String,      // fallback: single combined label (no k/v split)
  mono: { type: Boolean, default: true },
})
</script>

<template>
  <div class="flex items-center gap-2 py-1">
    <span class="dot rounded-full shrink-0" :class="[`dot--${dot || 'gray'}`, { 'dot--pulse': pulse }]"></span>
    <template v-if="k">
      <span style="font-size:11px; color:var(--hm-text-secondary)">{{ k }}</span>
      <span class="font-mono" style="font-size:11px; color:var(--hm-text-primary); margin-left:auto">{{ v }}</span>
    </template>
    <span v-else :class="mono ? 'font-mono' : ''" style="font-size:11px; color:var(--hm-text-secondary)">{{ text }}</span>
  </div>
</template>

<style scoped>
.dot { width: 6px; height: 6px; }
.dot--green  { background: var(--hm-personal); }
.dot--amber  { background: var(--hm-warning); }
.dot--red    { background: var(--hm-danger); }
.dot--gray   { background: var(--hm-text-tertiary); }
.dot--pulse  { animation: dot-pulse 1.4s ease-in-out infinite; }
@keyframes dot-pulse { 0%, 100% { opacity: 1; } 50% { opacity: .35; } }
</style>
