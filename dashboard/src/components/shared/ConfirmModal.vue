<script setup>
import { ref, onMounted, onBeforeUnmount, nextTick } from 'vue'

const props = defineProps({ title: String, body: String, confirmLabel: String, dangerous: Boolean })
const emit = defineEmits(['confirm', 'cancel'])

const modalRef = ref(null)

function trapFocus(e) {
  if (!modalRef.value) return
  const focusable = modalRef.value.querySelectorAll('button, [href], input, [tabindex]:not([tabindex="-1"])')
  const first = focusable[0]
  const last = focusable[focusable.length - 1]
  if (e.key === 'Tab') {
    if (e.shiftKey) {
      if (document.activeElement === first) { e.preventDefault(); last.focus() }
    } else {
      if (document.activeElement === last) { e.preventDefault(); first.focus() }
    }
  }
  if (e.key === 'Escape') emit('cancel')
}

onMounted(async () => {
  document.addEventListener('keydown', trapFocus)
  await nextTick()
  modalRef.value?.querySelector('button')?.focus()
})

onBeforeUnmount(() => {
  document.removeEventListener('keydown', trapFocus)
})
</script>

<template>
  <div class="fixed inset-0 z-[150] flex animate-fade items-center justify-center bg-overlay backdrop-blur-[4px]"
    @click.self="$emit('cancel')">
    <div ref="modalRef"
      role="dialog"
      aria-modal="true"
      aria-labelledby="confirm-modal-title"
      class="w-[440px] max-w-[calc(100vw-40px)] animate-modal-in rounded-xl border border-line bg-card p-6 shadow-2xl">
      <h3 id="confirm-modal-title" class="mb-2 font-headline text-[19px] font-bold tracking-[-0.025em] text-fg-strong">{{ title }}</h3>
      <p class="mb-5 mt-0 text-sm leading-[1.6] text-fg-muted">{{ body }}</p>
      <div class="flex justify-end gap-2.5">
        <button class="hm-btn hm-btn-default" @click="$emit('cancel')">Cancel</button>
        <button
          class="hm-btn"
          :class="dangerous ? 'hm-btn-danger' : 'hm-btn-primary'"
          @click="$emit('confirm')">
          {{ confirmLabel }}
        </button>
      </div>
    </div>
  </div>
</template>
