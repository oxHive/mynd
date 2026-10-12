<script setup>
defineProps({ label: String, disabled: Boolean })
const model = defineModel({ type: Boolean, required: true })
</script>

<template>
  <label class="relative inline-block h-[22px] w-10 shrink-0" :class="disabled ? 'cursor-not-allowed opacity-50' : 'cursor-pointer'">
    <input v-model="model" type="checkbox" role="switch" :aria-label="label" :disabled="disabled" class="peer sr-only" />
    <span class="absolute inset-0 rounded-pill bg-line-strong transition-colors duration-200 peer-checked:bg-accent peer-focus-visible:shadow-[0_0_0_3px_var(--accent-ring)]"></span>
    <span class="absolute left-[3px] top-[3px] h-4 w-4 rounded-circle bg-white shadow-sm transition-transform duration-200 peer-checked:translate-x-[18px]"></span>
  </label>
</template>
