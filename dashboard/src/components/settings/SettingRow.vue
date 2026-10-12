<script setup>
defineProps({ label: { type: String, required: true }, hint: String })
</script>

<template>
  <div class="flex flex-col items-start gap-2 py-3.5 @min-[520px]:flex-row @min-[520px]:flex-wrap @min-[520px]:items-center @min-[520px]:gap-x-4">
    <div class="shrink-0 font-headline text-[13.5px] font-semibold text-fg @min-[520px]:w-40">
      {{ label }}
      <div v-if="hint" class="mt-[3px] font-body text-xs font-normal text-fg-faint">{{ hint }}</div>
    </div>
    <div class="w-full min-w-0 flex-1 text-[13.5px] text-fg-muted @min-[520px]:w-auto"><slot /></div>
  </div>
</template>
