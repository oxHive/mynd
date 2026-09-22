import { defineStore } from 'pinia'
import { ref, watch } from 'vue'
import { readStored } from '../lib/localStore'

const STORAGE_KEY = 'mynd.fontScale'
export const MIN_PERCENT = 80
export const MAX_PERCENT = 130
export const DEFAULT_PERCENT = 100

// Almost every component here sets font-size in hardcoded px, not rem, so a
// CSS variable scale wouldn't reach most of the app. `zoom` scales the whole
// rendered tree (text, spacing, layout) together and actually reflows content
// (unlike `transform: scale`, which just visually stretches without
// adjusting layout), so it's the only lever that touches "anything else"
// as the user asked, without rewriting every inline font-size in the app.
export const useFontScaleStore = defineStore('fontScale', () => {
  const stored = parseInt(readStored(STORAGE_KEY), 10)
  const percent = ref(
    Number.isFinite(stored) && stored >= MIN_PERCENT && stored <= MAX_PERCENT ? stored : DEFAULT_PERCENT
  )

  watch(percent, (v) => {
    document.documentElement.style.zoom = `${v}%`
    localStorage.setItem(STORAGE_KEY, String(v))
  }, { immediate: true })

  function setPercent(next) {
    percent.value = Math.min(MAX_PERCENT, Math.max(MIN_PERCENT, Math.round(next)))
  }

  function reset() {
    setPercent(DEFAULT_PERCENT)
  }

  return { percent, setPercent, reset }
})
