<script setup>
defineProps({
  options: { type: Array, required: true }, // [{ value, label, icon? }]
  size: { type: String, default: 'md' },
  label: String,
})
const model = defineModel({ type: String, required: true })
</script>

<template>
  <div role="radiogroup" :aria-label="label" class="inline-flex gap-0.5 rounded-xl bg-chip" :class="size === 'sm' ? 'p-0.5' : 'p-[3px]'">
    <button
      v-for="o in options"
      :key="o.value"
      type="button"
      role="radio"
      :aria-checked="model === o.value"
      :title="o.title"
      class="inline-flex items-center gap-1.5 rounded-[6px] font-headline font-semibold transition-colors duration-150"
      :class="[
        size === 'sm' ? 'px-2.5 py-1 text-xs' : 'px-3 py-1.5 text-[12.5px]',
        model === o.value ? 'bg-card text-fg-strong shadow-sm' : 'text-fg-muted hover:text-fg',
      ]"
      @click="model = o.value"
    >
      <span v-if="o.icon" class="material-symbols-outlined text-[16px]!" aria-hidden="true">{{ o.icon }}</span>{{ o.label }}
    </button>
  </div>
</template>
