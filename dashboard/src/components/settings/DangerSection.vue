<script setup>
import { ref } from 'vue'
import DangerModal from './DangerModal.vue'
import DataRow from './DataRow.vue'
const showModal = ref(false)
</script>

<template>
  <div class="rounded-xl border border-danger-line bg-danger-tint px-[18px] py-1">
    <DataRow title="Clear all memories" desc="Permanently deletes all memories, edges, and feedback.">
      <button type="button" class="hm-btn hm-btn-danger" @click="showModal = true">
        Clear all <span class="material-symbols-outlined text-[16px]!" aria-hidden="true">warning</span>
      </button>
    </DataRow>
  </div>
  <DangerModal v-if="showModal" @close="showModal = false" />
</template>
