<script setup>
import { useUiStore } from '../../stores/ui.js'
const ui = useUiStore()
</script>

<template>
  <div role="status" aria-live="polite" class="fixed bottom-3 left-1/2 -translate-x-1/2 z-[200]">
    <Transition name="toast">
      <div v-if="ui.toast.visible"
        class="whitespace-nowrap rounded-md px-3.5 py-1.5 font-mono text-xs"
        style="background:var(--hm-bg-overlay); border:0.5px solid var(--hm-border-default); color:var(--hm-text-secondary)">
        {{ ui.toast.message }}
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.toast-enter-active, .toast-leave-active { transition: opacity 0.2s; }
.toast-enter-from, .toast-leave-to { opacity: 0; }
</style>
