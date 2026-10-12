<script setup>
import { onBeforeUnmount, onMounted } from 'vue'

defineProps({ label: String, width: { type: Number, default: 440 } })
const emit = defineEmits(['close'])

const onKey = (e) => { if (e.key === 'Escape') emit('close') }
onMounted(() => window.addEventListener('keydown', onKey))
onBeforeUnmount(() => window.removeEventListener('keydown', onKey))
</script>

<template>
  <Teleport to="body">
    <div class="fixed inset-0 z-[150] flex animate-fade items-center justify-center bg-overlay backdrop-blur-[4px]" @mousedown.self="emit('close')">
      <div role="dialog" aria-modal="true" :aria-label="label"
        class="max-w-[calc(100vw-40px)] animate-modal-in rounded-xl border border-line bg-card p-6 shadow-2xl"
        :style="{ width: width + 'px' }">
        <slot />
      </div>
    </div>
  </Teleport>
</template>
