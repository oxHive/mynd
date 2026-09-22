<script setup>
import { useFeedbackStore } from '../stores/feedback.js'
import ConflictCard from '../components/feedback/ConflictCard.vue'
import FeedbackCard from '../components/feedback/FeedbackCard.vue'
import EmptyState from '../components/shared/EmptyState.vue'

const fb = useFeedbackStore()
</script>

<template>
  <div class="flex flex-col flex-1 overflow-hidden">

    <!-- Tabs -->
    <div class="flex px-5 pt-4"
      style="border-bottom:0.5px solid var(--hm-border-subtle); background:var(--hm-bg-surface)">
      <button
        v-for="tab in ['conflicts','feedback']" :key="tab"
        @click="fb.activeTab = tab"
        class="px-3 pb-2.5 font-mono capitalize"
        :style="fb.activeTab===tab
          ? 'font-size:12px; color:var(--hm-text-primary); border-bottom:2px solid var(--hm-personal); cursor:pointer'
          : 'font-size:12px; color:var(--hm-text-tertiary); border-bottom:2px solid transparent; cursor:pointer'">
        {{ tab }}
        <span v-if="tab==='conflicts' && fb.conflicts.length" class="ml-1.5 font-mono"
          style="font-size:10px; background:var(--hm-warning-bg); color:var(--hm-warning); padding:1px 5px; border-radius:3px">
          {{ fb.conflicts.length }}
        </span>
      </button>
    </div>

    <!-- Content -->
    <div class="flex-1 overflow-y-auto px-6 py-5">
      <template v-if="fb.activeTab === 'conflicts'">
        <EmptyState v-if="!fb.conflicts.length" message="No conflicts."
          hint="Conflicts appear when a sync overwrites a local edit." />
        <ConflictCard v-for="c in fb.conflicts" :key="c.id" :conflict="c" />
      </template>
      <template v-else>
        <EmptyState v-if="!fb.feedbackItems.length" message="No open feedback."
          hint="Flag a memory with /memory-flag <id> to queue it here." />
        <FeedbackCard v-for="item in fb.feedbackItems" :key="item.id" :item="item" />
      </template>
    </div>
  </div>
</template>
