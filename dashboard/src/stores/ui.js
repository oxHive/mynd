import { defineStore } from 'pinia'
import { ref } from 'vue'
import { getStatus } from '../api/memories.js'

export const useUiStore = defineStore('ui', () => {
  const activeView = ref('analytics')
  const serverStatus = ref('checking') // 'checking'|'running'|'unreachable'|'syncing'|'sync_failed'
  const serverInfo = ref(null)
  const syncInfo = ref(null)
  const orgInfo = ref(null)
  const toast = ref({ message: '', visible: false })

  // Lets a view (currently: Settings > Tags) veto a top-level view switch
  // while it has unsaved edits. Set to a () => boolean function that
  // returns true if it's safe to navigate away (prompting the user itself
  // if needed), false to cancel the navigation. Only one guard at a time;
  // registered/cleared by the view that owns the unsaved state.
  const navigationGuard = ref(null)

  function registerNavigationGuard(fn) {
    navigationGuard.value = fn
  }

  function clearNavigationGuard() {
    navigationGuard.value = null
  }

  // Use this instead of assigning `activeView` directly from anywhere a
  // user-initiated navigation can happen (sidebar clicks, hash changes,
  // "back to memories" shortcuts) so the guard actually gets a chance to run.
  function requestActiveView(view) {
    if (view === activeView.value) return
    if (navigationGuard.value && !navigationGuard.value()) return
    activeView.value = view
  }

  let toastTimer = null

  function showToast(message, duration = 2200) {
    if (toastTimer) clearTimeout(toastTimer)
    toast.value = { message, visible: true }
    toastTimer = setTimeout(() => { toast.value.visible = false }, duration)
  }

  async function copyToClipboard(text) {
    await navigator.clipboard.writeText(text)
    showToast('Copied: ' + text)
  }

  async function pollServerStatus() {
    try {
      const data = await getStatus()
      serverStatus.value = 'running'
      serverInfo.value = data.info ?? data
      syncInfo.value = data.sync ?? null
      orgInfo.value = data.org ?? null
    } catch {
      serverStatus.value = 'unreachable'
    }
  }

  return {
    activeView, serverStatus, serverInfo, syncInfo, orgInfo, toast,
    registerNavigationGuard, clearNavigationGuard, requestActiveView,
    showToast, copyToClipboard, pollServerStatus,
  }
})
