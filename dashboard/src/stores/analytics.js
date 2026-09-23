import { defineStore } from 'pinia'
import { ref } from 'vue'
import * as api from '../api/sessionLogs.js'

export const useAnalyticsStore = defineStore('analytics', () => {
  const sessionLogs = ref([])
  const loadingLogs = ref(false)

  async function fetchSessionLogs() {
    loadingLogs.value = true
    try {
      const data = await api.getSessionLogs(50)
      sessionLogs.value = data.logs ?? []
    } finally {
      loadingLogs.value = false
    }
  }

  return { sessionLogs, loadingLogs, fetchSessionLogs }
})
