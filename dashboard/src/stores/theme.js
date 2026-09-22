import { defineStore } from 'pinia'
import { ref, computed, watch } from 'vue'
import { readStored } from '../lib/localStore'

const STORAGE_KEY = 'mynd.theme'
const MODES = ['system', 'light', 'dark']

export const useThemeStore = defineStore('theme', () => {
  const stored = readStored(STORAGE_KEY)
  const mode = ref(MODES.includes(stored) ? stored : 'system')

  const media = window.matchMedia('(prefers-color-scheme: light)')
  const systemPrefersLight = ref(media.matches)
  media.addEventListener('change', e => { systemPrefersLight.value = e.matches })

  const resolved = computed(() => (mode.value === 'system' ? (systemPrefersLight.value ? 'light' : 'dark') : mode.value))

  watch(resolved, v => document.documentElement.setAttribute('data-theme', v), { immediate: true })

  function setMode(next) {
    if (!MODES.includes(next)) return
    mode.value = next
    localStorage.setItem(STORAGE_KEY, next)
  }

  return { mode, resolved, setMode }
})
