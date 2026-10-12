<script setup>
import { ref, onBeforeUnmount, onMounted, nextTick } from 'vue'

const props = defineProps({
  label: String,
  width: { type: Number, default: 440 },
  // Ignore Escape and scrim clicks, e.g. while a destructive request is in flight.
  persistent: Boolean,
})
const emit = defineEmits(['close'])

const dialog = ref(null)
const FOCUSABLE = 'button:not([disabled]), [href], input:not([disabled]), textarea:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])'
let returnFocus = null

function requestClose() {
  if (!props.persistent) emit('close')
}

function onKey(e) {
  if (e.key === 'Escape') { requestClose(); return }
  if (e.key !== 'Tab' || !dialog.value) return
  const items = [...dialog.value.querySelectorAll(FOCUSABLE)]
  if (!items.length) { e.preventDefault(); return }
  const first = items[0], last = items[items.length - 1]
  if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last.focus() }
  else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus() }
  else if (!dialog.value.contains(document.activeElement)) { e.preventDefault(); first.focus() }
}

onMounted(async () => {
  returnFocus = document.activeElement
  window.addEventListener('keydown', onKey)
  await nextTick()
  // `autofocus` is ignored for elements inserted after page load, so honor it here.
  const target = dialog.value?.querySelector('[autofocus]') || dialog.value?.querySelector(FOCUSABLE)
  target?.focus()
})
onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKey)
  returnFocus?.focus?.()
})
</script>

<template>
  <Teleport to="body">
    <div class="fixed inset-0 z-[150] flex animate-fade items-center justify-center bg-overlay backdrop-blur-[4px]" @mousedown.self="requestClose">
      <div ref="dialog" role="dialog" aria-modal="true" :aria-label="label"
        class="max-w-[calc(100vw-40px)] animate-modal-in rounded-xl border border-line bg-card p-6 shadow-2xl"
        :style="{ width: width + 'px' }">
        <slot />
      </div>
    </div>
  </Teleport>
</template>
