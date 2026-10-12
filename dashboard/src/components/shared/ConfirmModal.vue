<script setup>
import Modal from './Modal.vue'

defineProps({ title: String, body: String, confirmLabel: String, dangerous: Boolean })
const emit = defineEmits(['confirm', 'cancel'])
</script>

<template>
  <Modal :label="title" @close="emit('cancel')">
    <h3 class="mb-2 font-headline text-[19px] font-bold tracking-[-0.025em] text-fg-strong">{{ title }}</h3>
    <p class="mb-5 mt-0 text-sm leading-[1.6] text-fg-muted">{{ body }}</p>
    <div class="flex justify-end gap-2.5">
      <button type="button" class="hm-btn hm-btn-default" @click="emit('cancel')">Cancel</button>
      <button type="button" class="hm-btn" :class="dangerous ? 'hm-btn-danger' : 'hm-btn-primary'" @click="emit('confirm')">
        {{ confirmLabel }}
      </button>
    </div>
  </Modal>
</template>
