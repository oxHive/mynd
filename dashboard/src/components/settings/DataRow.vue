<script setup>
defineProps({ title: { type: String, required: true }, desc: String, meta: String })
</script>

<template>
  <div class="flex items-center gap-4 py-4">
    <div class="min-w-0">
      <div class="font-headline text-[13.5px] font-semibold text-fg">{{ title }}</div>
      <div v-if="desc" class="mt-[3px] text-[12.5px] text-fg-muted">{{ desc }}</div>
      <div v-if="meta" class="mt-1.5 font-headline text-[11px] tabular-nums text-fg-faint">{{ meta }}</div>
    </div>
    <div class="ml-auto shrink-0"><slot /></div>
  </div>
</template>
