<script setup>
import { computed } from 'vue'
import { useUiStore } from '../../stores/ui.js'
import { useFeedbackStore } from '../../stores/feedback.js'
import { useGraphStore } from '../../stores/graph.js'
import { useUpdateStore } from '../../stores/update.js'
import StatusRow from './StatusRow.vue'
import oxhiveMark from '../../assets/oxhive-mark.png'
import { BASE } from '../../api/client.js'

const ui = useUiStore()
const feedback = useFeedbackStore()
const graph = useGraphStore()
const update = useUpdateStore()

const feedbackCount = computed(() =>
  feedback.conflicts.length + feedback.feedbackItems.length
)

const navItems = [
  { id: 'analytics', label: 'Analytics', icon: 'analytics' },
  { id: 'memories', label: 'Memories', icon: 'memories' },
  { id: 'graph', label: 'Graph', icon: 'graph' },
  { id: 'feedback', label: 'Feedback', icon: 'feedback' },
  { id: 'settings', label: 'Settings', icon: 'settings' },
]

// Pending connection suggestions: badged on the two pages that surface them
// (both show the review bar), same treatment as the feedback count.
function navBadgeCount(id) {
  if (id === 'feedback') return feedbackCount.value
  if (id === 'memories' || id === 'graph') return graph.pendingEdges.length
  return 0
}

const statusDot = computed(() => {
  if (ui.serverStatus === 'unreachable') return 'red'
  if (ui.serverStatus === 'sync_failed') return 'red'
  if (ui.serverStatus === 'syncing') return 'amber'
  return 'green'
})

const memoryCount = computed(() => {
  if (!ui.serverInfo) return '—'
  const primary = ui.serverInfo?.memory_count ?? ui.serverInfo?.memoryCount ?? 0
  const org = ui.orgInfo?.count ?? 0
  return primary + org
})

const serverAddress = computed(() => (BASE || 'http://localhost:3456').replace(/^https?:\/\//, ''))

const syncInfo = computed(() => ui.syncInfo)

const syncStatusText = computed(() => {
  if (!syncInfo.value?.enabled) return null
  const last = syncInfo.value?.last_synced_at
  if (!last) return 'not yet synced'
  const diffSec = Math.floor(Date.now() / 1000) - last
  if (diffSec < 60) return 'synced · just now'
  const diffMin = Math.floor(diffSec / 60)
  return `synced · ${diffMin}m ago`
})

const syncDot = computed(() => {
  if (!syncInfo.value?.enabled) return null
  const last = syncInfo.value?.last_synced_at
  if (!last) return 'gray'
  const diffSec = Math.floor(Date.now() / 1000) - last
  return diffSec > 600 ? 'amber' : 'green'
})

const conflictCount = computed(() => syncInfo.value?.conflict_count ?? 0)
const conflictDot = computed(() => (conflictCount.value > 0 ? 'amber' : 'green'))

const orgInfo = computed(() => ui.orgInfo)

const orgSyncStatusText = computed(() => {
  if (!orgInfo.value?.configured) return null
  if (!orgInfo.value?.enabled) return 'configured, sync disabled'
  const last = orgInfo.value?.last_synced_at
  if (!last) return 'not yet synced'
  const diffSec = Math.floor(Date.now() / 1000) - last
  if (diffSec < 60) return 'synced · just now'
  const diffMin = Math.floor(diffSec / 60)
  return `synced · ${diffMin}m ago`
})

const orgSyncDot = computed(() => {
  if (!orgInfo.value?.configured || !orgInfo.value?.enabled) return 'gray'
  const last = orgInfo.value?.last_synced_at
  if (!last) return 'gray'
  const diffSec = Math.floor(Date.now() / 1000) - last
  return diffSec > 600 ? 'amber' : 'green'
})
</script>

<template>
  <nav class="flex flex-col shrink-0 h-full"
    style="width:200px; background:var(--hm-bg-surface); border-right:0.5px solid var(--hm-border-subtle)">

    <!-- Logo -->
    <div class="px-5 pt-6 pb-7 flex items-center justify-start gap-2"
      style="border-bottom:0.5px solid var(--hm-border-subtle)">
      <div class="flex items-center" style="gap:4px">
        <svg width="24" height="24" viewBox="0 0 16 16" aria-hidden="true">
          <polygon points="8,1.5 13.6,4.75 13.6,11.25 8,14.5 2.4,11.25 2.4,4.75"
            fill="none" stroke="var(--hm-accent)" stroke-width="1.2" />
          <circle cx="8" cy="8" r="2" fill="var(--hm-accent)" />
        </svg>
        <div style="font-size:19px; font-weight:600; letter-spacing:-0.01em; color:var(--hm-text-primary); line-height:1">Mynd</div>
      </div>
      <span class="font-mono self-end" style="font-size:10px; color:var(--hm-text-tertiary); line-height:1">
        v{{ ui.serverInfo?.version || '—' }}
      </span>
    </div>

    <!-- Nav -->
    <ul class="flex flex-col py-3">
      <li v-for="item in navItems" :key="item.id">
        <button
          @click="ui.requestActiveView(item.id)"
          class="nav-item"
          :class="{ 'nav-item--active': ui.activeView === item.id }"
          :aria-current="ui.activeView === item.id ? 'page' : undefined"
        >
          <span class="nav-item__left">
            <svg class="nav-item__icon" width="16" height="16" :viewBox="item.icon === 'settings' ? '0 0 24 24' : '0 0 16 16'" fill="none" aria-hidden="true">
              <template v-if="item.icon === 'analytics'">
                <path d="M2.5 13.5V8.5M8 13.5V4.5M13.5 13.5V6.5" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" />
              </template>
              <template v-else-if="item.icon === 'memories'">
                <ellipse cx="8" cy="3.6" rx="5" ry="2.1" stroke="currentColor" stroke-width="1.3" />
                <path d="M3 3.6V8c0 1.16 2.24 2.1 5 2.1s5-.94 5-2.1V3.6" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" />
                <path d="M3 8v4.4c0 1.16 2.24 2.1 5 2.1s5-.94 5-2.1V8" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" />
              </template>
              <template v-else-if="item.icon === 'graph'">
                <circle cx="11.5" cy="3.5" r="1.7" stroke="currentColor" stroke-width="1.3" />
                <circle cx="11.5" cy="12.5" r="1.7" stroke="currentColor" stroke-width="1.3" />
                <circle cx="4" cy="8" r="1.7" stroke="currentColor" stroke-width="1.3" />
                <path d="M5.5 7.1l4.4-2.7M5.5 8.9l4.4 2.7" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" />
              </template>
              <template v-else-if="item.icon === 'feedback'">
                <path d="M3.5 2v12" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" />
                <path d="M3.5 2.8h7.3c.8 0 1.2.9.7 1.5l-1.8 2.2 1.8 2.2c.5.6.1 1.5-.7 1.5H3.5" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" />
              </template>
              <template v-else-if="item.icon === 'settings'">
                <path d="M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z" stroke="currentColor" stroke-width="1.8" />
                <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"
                  stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" />
              </template>
            </svg>
            <span>{{ item.label }}</span>
          </span>
          <span v-if="navBadgeCount(item.id) > 0"
            class="nav-item__badge font-mono">
            {{ navBadgeCount(item.id) }}
          </span>
        </button>
      </li>
    </ul>

    <!-- Update available + status (push to bottom) -->
    <div class="mt-auto">
      <div v-if="update.available && update.platformSupported" class="px-2 pb-2">
        <button
          @click="update.changelogOpen = true"
          class="nav-item"
          style="border-radius:6px; flex-wrap:nowrap"
        >
          <span style="white-space:nowrap; overflow:hidden; text-overflow:ellipsis; min-width:0">Update available</span>
          <span
            class="font-mono rounded-sm px-1.5 py-0.5"
            style="font-size:10px; background:var(--hm-warning-bg); color:var(--hm-warning); white-space:nowrap; flex-shrink:0">
            v{{ update.latestVersion }}
          </span>
        </button>
      </div>
      <div class="px-5 pb-5 pt-4"
        style="border-top:0.5px solid var(--hm-border-subtle)">
        <StatusRow :dot="statusDot" k="server" :v="serverAddress" />
        <StatusRow v-if="syncStatusText" :dot="syncDot" pulse k="sync" :v="syncStatusText" class="mt-1" />
        <StatusRow v-if="orgInfo?.configured" :dot="orgSyncDot" pulse k="org" :v="orgSyncStatusText" class="mt-1" />
        <StatusRow :dot="conflictDot" k="conflicts" :v="String(conflictCount)" class="mt-1" />
        <StatusRow dot="gray" :text="`${memoryCount} memories`" class="mt-1" />
      </div>
    </div>

    <!-- Footer -->
    <div class="footer">
      <img class="footer__mark" :src="oxhiveMark" alt="" aria-hidden="true" width="18" height="18" />
      <span class="footer__word">OxHive</span>
    </div>
  </nav>
</template>

<style scoped>
.nav-item {
  width: calc(100% - 16px);
  margin: 1px 8px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 9px 12px;
  font-size: 13px;
  text-align: left;
  color: var(--hm-text-secondary);
  background: transparent;
  border: none;
  border-radius: 8px;
  cursor: pointer;
  transition: background 0.1s, color 0.1s;
}

.nav-item__left {
  display: flex;
  align-items: center;
  gap: 10px;
}

.nav-item__icon {
  flex-shrink: 0;
  color: var(--hm-text-tertiary);
  transition: color 0.1s;
}

.nav-item:hover,
.nav-item:focus-visible {
  background: var(--hm-bg-elevated);
  color: var(--hm-text-primary);
  outline: none;
}

.nav-item:hover .nav-item__icon,
.nav-item:focus-visible .nav-item__icon {
  color: var(--hm-text-primary);
}

.nav-item:focus-visible {
  outline: 2px solid var(--hm-accent);
  outline-offset: -2px;
}

.nav-item--active {
  background: var(--hm-bg-elevated);
  color: var(--hm-text-primary);
  font-weight: 500;
}

.nav-item--active .nav-item__icon {
  color: var(--hm-text-primary);
}

.nav-item__badge {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 18px;
  height: 18px;
  padding: 0 5px;
  border-radius: 999px;
  font-size: 10px;
  line-height: 1;
  background: var(--hm-warning-bg);
  color: var(--hm-warning);
}

.footer {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 5px;
  padding: 14px 20px 16px;
}

.footer__mark {
  display: block;
  filter: brightness(0) invert(1);
}

:root[data-theme="light"] .footer__mark {
  filter: none;
}

.footer__word {
  font-family: "Hanken Grotesk", var(--hm-font-sans);
  font-size: 15px;
  font-weight: 800;
  letter-spacing: -0.02em;
  color: var(--hm-text-primary);
}
</style>
