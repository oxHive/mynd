<script setup>
import { computed } from 'vue'
import { useMemoriesStore } from '../stores/memories.js'
import { useAnalyticsStore } from '../stores/analytics.js'
import { useUiStore } from '../stores/ui.js'
import BarChart from '../components/analytics/BarChart.vue'
import SessionLogRow from '../components/analytics/SessionLogRow.vue'
import EmptyState from '../components/shared/EmptyState.vue'

const memories = useMemoriesStore()
const analytics = useAnalyticsStore()
const ui = useUiStore()

const totalMemories = computed(() => ui.serverInfo?.memory_count ?? memories.all.length)
const totalTags = computed(() => analytics.tagCounts.length)
const totalProjects = computed(() => analytics.projectCounts.length)
const addedThisWeek = computed(() => {
  const weekAgo = Date.now() / 1000 - 7 * 24 * 60 * 60
  return memories.all.filter(m => m.created_at >= weekAgo).length
})

const topTags = computed(() => analytics.tagCounts.slice(0, 10))
const typeColor = (d) => {
  if (d.type === 'preference') return 'var(--hm-personal)'
  if (d.type === 'project') return 'var(--hm-workspace)'
  return 'var(--hm-accent)'
}
</script>

<template>
  <div class="flex-1 overflow-y-auto px-8 py-8">
    <h2 class="mb-6 font-medium" style="font-size:16px; color:var(--hm-text-primary)">Analytics</h2>

    <div class="grid grid-cols-4 gap-4 mb-6">
      <div class="analytics-tile">
        <div class="analytics-tile__label">Total memories</div>
        <div class="analytics-tile__value">{{ totalMemories }}</div>
      </div>
      <div class="analytics-tile">
        <div class="analytics-tile__label">Distinct tags</div>
        <div class="analytics-tile__value">{{ totalTags }}</div>
      </div>
      <div class="analytics-tile">
        <div class="analytics-tile__label">Projects</div>
        <div class="analytics-tile__value">{{ totalProjects }}</div>
      </div>
      <div class="analytics-tile">
        <div class="analytics-tile__label">Added last 7 days</div>
        <div class="analytics-tile__value">{{ addedThisWeek }}</div>
      </div>
    </div>

    <div class="grid grid-cols-2 gap-4 mb-6">
      <div class="analytics-panel">
        <h3 class="analytics-panel__title">Top tags</h3>
        <BarChart v-if="topTags.length" :data="topTags" labelKey="tag" valueKey="count" />
        <EmptyState v-else message="No tags yet" />
      </div>
      <div class="analytics-panel">
        <h3 class="analytics-panel__title">Memory types</h3>
        <BarChart v-if="analytics.typeCounts.length" :data="analytics.typeCounts" labelKey="type" valueKey="count" :color="typeColor" />
        <EmptyState v-else message="No memories yet" />
      </div>
      <div class="analytics-panel">
        <h3 class="analytics-panel__title">By project</h3>
        <BarChart v-if="analytics.projectCounts.length" :data="analytics.projectCounts" labelKey="project" valueKey="count" />
        <EmptyState v-else message="No project-tagged memories yet" hint="Add a project:* tag to a memory to see it here." />
      </div>
      <div class="analytics-panel">
        <h3 class="analytics-panel__title">Activity by day</h3>
        <BarChart v-if="analytics.activityByDay.length" :data="analytics.activityByDay" labelKey="day" valueKey="count" />
        <EmptyState v-else message="No activity recorded yet" />
      </div>
    </div>

    <div class="analytics-panel">
      <h3 class="analytics-panel__title">Recall sessions</h3>
      <div v-if="analytics.sessionLogs.length">
        <SessionLogRow v-for="log in analytics.sessionLogs" :key="log.id" :log="log" />
      </div>
      <EmptyState v-else
        message="No session-start runs logged yet"
        hint="This fills in once a Claude Code session with Mynd configured runs its session-start hook." />
    </div>
  </div>
</template>

<style scoped>
.analytics-tile {
  background: var(--hm-bg-elevated);
  border: 0.5px solid var(--hm-border-default);
  border-radius: 8px;
  padding: 16px 20px;
}
.analytics-tile__label {
  font-size: 11px;
  color: var(--hm-text-tertiary);
  margin-bottom: 4px;
}
.analytics-tile__value {
  font-size: 24px;
  font-weight: 600;
  color: var(--hm-text-primary);
  font-variant-numeric: tabular-nums;
}
.analytics-panel {
  background: var(--hm-bg-elevated);
  border: 0.5px solid var(--hm-border-default);
  border-radius: 8px;
  padding: 20px;
}
.analytics-panel__title {
  font-size: 12px;
  color: var(--hm-text-secondary);
  margin-bottom: 14px;
}
</style>
