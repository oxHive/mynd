<script setup>
import { useFeedbackStore } from '../../stores/feedback.js'
import LayerBadge from '../shared/LayerBadge.vue'

const props = defineProps({ conflict: Object })
const fb = useFeedbackStore()
</script>

<template>
  <div class="rounded-lg p-5 mb-4"
    style="background:var(--hm-bg-elevated); border:0.5px solid var(--hm-border-default)">

    <div class="flex items-start justify-between mb-4">
      <div>
        <span class="font-mono" style="font-size:10px; color:var(--hm-text-tertiary)">{{ conflict.id }}</span>
        <p class="mt-1" style="font-size:13px; font-weight:500; color:var(--hm-text-primary)">{{ conflict.title || 'Conflict' }}</p>
      </div>
      <LayerBadge v-if="conflict.layer" :layer="conflict.layer" class="shrink-0" />
    </div>

    <!-- Two-column diff -->
    <div class="grid grid-cols-2 gap-3 mb-4">
      <div class="rounded p-3" style="background:var(--hm-danger-bg); border:0.5px solid var(--hm-danger-border)">
        <p class="font-mono mb-1.5" style="font-size:9px; color:var(--hm-danger)">YOUR LOCAL VERSION</p>
        <p style="font-size:11px; color:var(--hm-text-secondary)">{{ conflict.local }}</p>
      </div>
      <div class="rounded p-3" style="background:var(--hm-bg-overlay); border:0.5px solid var(--hm-border-default)">
        <p class="font-mono mb-1.5" style="font-size:9px; color:var(--hm-text-tertiary)">CURRENT (FROM REMOTE)</p>
        <p style="font-size:11px; color:var(--hm-text-secondary)">{{ conflict.current }}</p>
      </div>
    </div>

    <div class="flex items-center gap-2">
      <button class="hm-btn hm-btn-primary hm-btn-sm"
        @click="fb.resolveConflict(conflict.id, 'keep_remote')">Keep remote</button>
      <button class="hm-btn hm-btn-default hm-btn-sm"
        @click="fb.resolveConflict(conflict.id, 'keep_local')">Restore local</button>
    </div>
  </div>
</template>
