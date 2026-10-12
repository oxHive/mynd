import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { getConfigFile, saveConfigFile, restartServer } from '../api/settings.js'
import { useUiStore } from './ui.js'

// Draft of the global config.toml as the dashboard edits it. `values` holds
// only what the file itself sets ({ section: { key: value } }); a removed
// key falls back to mynd's default. API keys are write-only: the server
// only reports where each comes from, and `apiKeyDrafts` holds a
// replacement (string) or a clear (null) until save.
export const useConfigFileStore = defineStore('configFile', () => {
  const values = ref({})
  const saved = ref('{}')
  const apiKeys = ref({})
  const apiKeyDrafts = ref({})
  const editable = ref(true)
  const path = ref('')
  const restartRequired = ref(false)
  const restarting = ref(false)
  const loaded = ref(false)
  const loadError = ref(null) // null | 'forbidden' | 'failed'

  const locked = computed(() => !editable.value)
  const isDirty = computed(() =>
    JSON.stringify(values.value) !== saved.value || Object.keys(apiKeyDrafts.value).length > 0)

  async function fetch() {
    try {
      const res = await getConfigFile()
      values.value = res.values
      saved.value = JSON.stringify(res.values)
      apiKeys.value = res.api_keys
      apiKeyDrafts.value = {}
      editable.value = res.editable
      path.value = res.path
      restartRequired.value = res.restart_required
      loadError.value = null
      loaded.value = true
    } catch (e) {
      loadError.value = e.status === 403 ? 'forbidden' : 'failed'
    }
  }

  const get = (section, key) => values.value[section]?.[key]

  function set(section, key, v) {
    const s = (values.value[section] ??= {})
    const empty = v === '' || v === null || v === undefined || (Array.isArray(v) && !v.length)
    if (empty) delete s[key]
    else s[key] = v
  }

  // Changed keys only, as { section: { key: value | null } }.
  function patch() {
    const before = JSON.parse(saved.value)
    const out = {}
    for (const section of new Set([...Object.keys(before), ...Object.keys(values.value)])) {
      const a = before[section] ?? {}, b = values.value[section] ?? {}
      for (const key of new Set([...Object.keys(a), ...Object.keys(b)])) {
        if (JSON.stringify(a[key]) !== JSON.stringify(b[key])) (out[section] ??= {})[key] = b[key] ?? null
      }
    }
    for (const [section, key] of Object.entries(apiKeyDrafts.value)) (out[section] ??= {}).api_key = key
    return out
  }

  async function save() {
    await saveConfigFile(patch())
    await fetch()
  }

  function discard() {
    values.value = JSON.parse(saved.value)
    apiKeyDrafts.value = {}
  }

  // Resolves true once the restarted server answers, false after ~30s (e.g.
  // the port changed and the dashboard is now pointed at the old address).
  async function restart() {
    const ui = useUiStore()
    restarting.value = true
    // The server may drop the connection before the response flushes.
    await restartServer().catch(() => {})
    try {
      for (let i = 0; i < 20; i++) {
        await new Promise(r => setTimeout(r, 1500))
        await ui.pollServerStatus()
        if (ui.serverStatus === 'running') {
          await fetch()
          return true
        }
      }
      return false
    } finally {
      restarting.value = false
    }
  }

  return {
    values, apiKeys, apiKeyDrafts, editable, locked, path, restartRequired, restarting, loaded, loadError,
    isDirty, fetch, get, set, patch, save, discard, restart,
  }
})
