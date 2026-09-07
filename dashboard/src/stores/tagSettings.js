import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { getTagSettings, saveTagSettings } from '../api/settings.js'

export const useTagSettingsStore = defineStore('tagSettings', () => {
  const namespaces = ref({})
  const loaded = ref(false)
  // Which namespace names are predefined (built into Mynd) vs
  // user-created. Drives the "predefined" label and hides the Remove
  // button in the settings UI. Authoritative list comes from the backend
  // (default_tag_namespaces()), not duplicated here.
  const predefined = ref([])
  // Whether the predefined-namespace guard is active. Set false via
  // [tags] guard_predefined_namespaces = false in the global mynd
  // config to allow editing/deleting predefined namespaces again.
  const guardPredefinedNamespaces = ref(true)
  // Snapshot of `namespaces` as last fetched/saved, used to detect unsaved
  // edits (compared by value, not reference, since namespaces is mutated
  // in place by the settings UI).
  const savedSnapshot = ref('{}')

  const isDirty = computed(() => JSON.stringify(namespaces.value) !== savedSnapshot.value)

  function isPredefined(name) {
    return predefined.value.includes(name)
  }

  async function fetchNamespaces() {
    const res = await getTagSettings()
    namespaces.value = res.namespaces
    predefined.value = res.predefined
    guardPredefinedNamespaces.value = res.guard_predefined_namespaces
    savedSnapshot.value = JSON.stringify(namespaces.value)
    loaded.value = true
  }

  async function save() {
    await saveTagSettings(namespaces.value)
    savedSnapshot.value = JSON.stringify(namespaces.value)
  }

  function namespaceFor(tag) {
    const idx = tag.indexOf(':')
    if (idx === -1) return null
    const ns = tag.slice(0, idx)
    return namespaces.value[ns] ? ns : null
  }

  const DEFAULT_TAG_COLOR = '#8a8f98'

  function colorFor(tag) {
    const ns = namespaceFor(tag)
    return ns ? namespaces.value[ns].color : DEFAULT_TAG_COLOR
  }

  return {
    namespaces, loaded, isDirty, predefined, guardPredefinedNamespaces, isPredefined,
    fetchNamespaces, save, namespaceFor, colorFor,
  }
})
