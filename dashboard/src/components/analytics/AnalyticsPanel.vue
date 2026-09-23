<script setup>
defineProps({
  title: { type: String, required: true },
  hint: { type: String, default: '' },
})
</script>

<template>
  <section class="rounded-lg min-w-0 overflow-hidden" style="background:var(--hm-bg-elevated); border:0.5px solid var(--hm-border-default); padding:18px 20px 20px">
    <header class="flex items-start gap-4 mb-4">
      <div class="flex-1 min-w-0">
        <h2 style="font-size:15px; letter-spacing:-0.02em; color:var(--hm-text-primary)">{{ title }}</h2>
        <div v-if="hint" class="mt-0.5" style="font-size:11.5px; color:var(--hm-text-tertiary)">{{ hint }}</div>
      </div>
      <slot name="right" />
    </header>
    <slot />
  </section>
</template>
