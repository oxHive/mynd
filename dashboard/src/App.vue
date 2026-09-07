<script setup>
import { onMounted, onBeforeUnmount, watch } from 'vue'
import { useUiStore } from './stores/ui.js'
import { useMemoriesStore } from './stores/memories.js'
import { useGraphStore } from './stores/graph.js'
import { useFeedbackStore } from './stores/feedback.js'
import { useTagSettingsStore } from './stores/tagSettings.js'
import { useThemeStore } from './stores/theme.js'
import { useFontScaleStore } from './stores/fontScale.js'
import { useAnalyticsStore } from './stores/analytics.js'
import { useSuggestStore } from './stores/suggest.js'
import { useUpdateStore } from './stores/update.js'
import { BASE } from './api/client.js'
import AppSidebar from './components/sidebar/AppSidebar.vue'
import SuggestPanel from './components/graph/SuggestPanel.vue'
import UpdateModal from './components/update/UpdateModal.vue'
import Toast from './components/shared/Toast.vue'
import AnalyticsView from './views/AnalyticsView.vue'
import MemoriesView from './views/MemoriesView.vue'
import GraphView from './views/GraphView.vue'
import FeedbackView from './views/FeedbackView.vue'
import SettingsView from './views/SettingsView.vue'

const ui = useUiStore()
const memories = useMemoriesStore()
const graph = useGraphStore()
const fb = useFeedbackStore()
const tagSettings = useTagSettingsStore()
const analytics = useAnalyticsStore()
const suggest = useSuggestStore()
const update = useUpdateStore()
useThemeStore() // applies data-theme to <html> as soon as the store is created
useFontScaleStore() // applies zoom to <html> as soon as the store is created

const VIEWS = ['analytics', 'memories', 'graph', 'feedback', 'settings']
const apiBase = window.MYND_API || 'http://localhost:3456'
let pollInterval
let eventSource

function applyHash() {
  const h = location.hash.replace('#/', '')
  if (VIEWS.includes(h)) ui.requestActiveView(h)
}

watch(() => ui.activeView, v => { location.hash = '#/' + v })

onMounted(async () => {
  applyHash()
  window.addEventListener('hashchange', applyHash)

  await ui.pollServerStatus()

  if (ui.serverStatus !== 'unreachable') {
    await Promise.all([
      memories.fetchAll(),
      graph.fetchEdges(),
      fb.fetchConflicts(),
      fb.fetchFeedback(),
      tagSettings.fetchNamespaces(),
      analytics.fetchSessionLogs(),
      suggest.hydrate(),
      update.refresh(),
    ])
  }

  pollInterval = setInterval(() => ui.pollServerStatus(), 30_000)

  if (ui.serverStatus !== 'unreachable') {
    // Silently reflects memories/edges changed elsewhere (e.g. via MCP tool
    // calls) without a visible reload — the browser reconnects on its own
    // if the connection drops.
    eventSource = new EventSource(BASE + '/api/v1/events')
    eventSource.onmessage = (e) => {
      let data
      try { data = JSON.parse(e.data) } catch { data = { type: 'changed' } }
      if (data.type === 'suggest_session') suggest.handleEvent(data)
      if (data.type === 'changed' || data.type === 'suggest_session') {
        memories.refreshSilently()
        graph.fetchEdges()
      }
      if (data.type === 'update_available' || data.type === 'update_progress' || data.type === 'update_failed') {
        update.handleEvent(data)
      }
    }
  }
})

onBeforeUnmount(() => {
  clearInterval(pollInterval)
  eventSource?.close()
  window.removeEventListener('hashchange', applyHash)
})
</script>

<template>
  <div class="hm-shell">

    <!-- First-load loader while we confirm the backend is reachable -->
    <div v-if="ui.serverStatus === 'checking'"
      class="flex flex-col items-center justify-center w-full gap-3"
      style="color:var(--hm-text-tertiary)">
      <div class="hm-hex hm-skeleton" style="width:28px; height:28px"></div>
      <p style="font-size:12px">Connecting to Mynd server…</p>
    </div>

    <!-- Full-screen error when server unreachable on first load -->
    <div v-else-if="ui.serverStatus === 'unreachable' && !memories.all.length"
      class="flex flex-col items-center justify-center w-full gap-4"
      style="color:var(--hm-text-secondary)">
      <p style="font-size:14px">Cannot connect to Mynd server at
        <code class="font-mono">{{ apiBase }}</code>.</p>
      <p style="font-size:12px; color:var(--hm-text-tertiary)">Run <code class="font-mono">mynd up</code> and then retry.</p>
      <button class="hm-btn hm-btn-default mt-2" @click="ui.pollServerStatus().then(() => memories.fetchAll())">
        Retry
      </button>
    </div>

    <template v-else>
      <AppSidebar />
      <main class="flex flex-1 overflow-hidden">
        <AnalyticsView v-show="ui.activeView === 'analytics'" />
        <MemoriesView v-show="ui.activeView === 'memories'" />
        <GraphView v-show="ui.activeView === 'graph'" />
        <FeedbackView v-show="ui.activeView === 'feedback'" />
        <SettingsView v-show="ui.activeView === 'settings'" />
      </main>
      <!-- Global: available from any page whenever there are pending
           suggestions to review, not just the Graph view. -->
      <SuggestPanel v-if="suggest.panelOpen" />
      <UpdateModal
        v-if="update.changelogOpen || update.status === 'updating'"
        @close="update.changelogOpen = false"
      />
    </template>

    <Toast />
  </div>
</template>
