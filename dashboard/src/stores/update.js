import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { getUpdateState, applyUpdate } from '../api/update.js'

export const useUpdateStore = defineStore('update', () => {
  const available = ref(false)
  const platformSupported = ref(true)
  const applyEnabled = ref(true)
  const upgradeCommand = ref(null) // set for Homebrew/cargo installs, which can't self-upgrade
  const status = ref('idle') // 'idle'|'checking'|'updating'|'failed'
  const currentVersion = ref(null)
  const latestVersion = ref(null)
  const releaseNotesMd = ref('')
  const releaseUrl = ref(null)
  const updateStartedAt = ref(null) // unix seconds, echoed by the server — anchors the elapsed-time counter across reloads
  const error = ref(null)
  const changelogOpen = ref(false)

  async function refresh() {
    try {
      const data = await getUpdateState()
      available.value = !!data.available
      platformSupported.value = !!data.platform_supported
      applyEnabled.value = data.apply_enabled ?? true
      upgradeCommand.value = data.upgrade_command ?? null
      status.value = data.status ?? 'idle'
      currentVersion.value = data.current_version ?? null
      latestVersion.value = data.latest_version ?? null
      releaseNotesMd.value = data.release_notes_md ?? ''
      releaseUrl.value = data.release_url ?? null
      updateStartedAt.value = data.update_started_at ?? null
      error.value = data.error ?? null
    } catch {
      // dashboard may be talking to a server that predates this feature, or
      // is momentarily unreachable — leave state as-is, next poll will retry
    }
  }

  // When set, the dashboard can't upgrade this install and shows this
  // command instead of the Update button.
  const manualCommand = computed(() =>
    upgradeCommand.value ?? (applyEnabled.value ? null : 'mynd upgrade'))

  async function startUpdate() {
    status.value = 'updating'
    error.value = null
    await applyUpdate()
  }

  function handleEvent(data) {
    if (data.type === 'update_available') {
      available.value = true
      latestVersion.value = data.latest_version ?? latestVersion.value
      releaseUrl.value = data.release_url ?? releaseUrl.value
    } else if (data.type === 'update_progress' && data.status === 'updating') {
      status.value = 'updating'
      updateStartedAt.value = data.started_at ?? updateStartedAt.value
    } else if (data.type === 'update_failed') {
      status.value = 'failed'
      error.value = data.error ?? 'update failed'
    }
  }

  return {
    available,
    platformSupported,
    manualCommand,
    status,
    currentVersion,
    latestVersion,
    releaseNotesMd,
    releaseUrl,
    updateStartedAt,
    error,
    changelogOpen,
    refresh,
    startUpdate,
    handleEvent,
  }
})
