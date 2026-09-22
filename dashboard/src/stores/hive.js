import { defineStore } from 'pinia'
import { ref } from 'vue'
import { getHiveStatus, getTrustedNetworks } from '../api/hive.js'
import { useUiStore } from './ui.js'

export const useHiveStore = defineStore('hive', () => {
  const enabled = ref(false)
  const identity = ref(null)
  const syncPort = ref(null)
  const pendingConflictCount = ref(0)
  const roster = ref([])
  const trustedNetworks = ref([])
  const currentNetwork = ref(null)
  const loaded = ref(false)
  const loadError = ref(null)

  // Tracks each peer's online flag from the last fetch so a
  // `hive_peer_status_changed` SSE event can toast only on an actual flip,
  // not on every ping-loop tick that leaves online/offline unchanged.
  let lastOnlineByDevice = new Map()

  async function fetchStatus() {
    try {
      const data = await getHiveStatus()
      enabled.value = data.enabled
      identity.value = data.identity ?? null
      syncPort.value = data.sync_port ?? null
      pendingConflictCount.value = data.pending_conflict_count ?? 0
      roster.value = data.roster ?? []
      loaded.value = true
      loadError.value = null
    } catch (e) {
      // dashboard may be talking to a server that's mid-restart (the
      // enable-toggle flow) or has hive unavailable — leave state as-is,
      // caller decides whether to retry
      loadError.value = e.status === 403 ? 'forbidden' : null
    }
  }

  async function fetchTrustedNetworks() {
    try {
      const data = await getTrustedNetworks()
      trustedNetworks.value = data.trusted ?? []
      currentNetwork.value = data.current_network ?? null
    } catch {
      // same tolerance as fetchStatus
    }
  }

  // Seeds the online-flag snapshot after the initial fetch so the first
  // `hive_peer_status_changed` event compares against real prior state
  // instead of toasting for every peer on the very first flip it sees.
  function primeOnlineSnapshot() {
    lastOnlineByDevice = new Map(roster.value.map(p => [p.device_id, p.online]))
  }

  async function handlePeerStatusChanged() {
    const previous = lastOnlineByDevice
    await fetchStatus()
    const ui = useUiStore()
    for (const peer of roster.value) {
      const wasOnline = previous.get(peer.device_id)
      if (wasOnline !== undefined && wasOnline !== peer.online) {
        ui.showToast(`${peer.name} is now ${peer.online ? 'online' : 'offline'}`)
      }
    }
    primeOnlineSnapshot()
  }

  return {
    enabled, identity, syncPort, pendingConflictCount, roster,
    trustedNetworks, currentNetwork, loaded, loadError,
    fetchStatus, fetchTrustedNetworks, primeOnlineSnapshot, handlePeerStatusChanged,
  }
})
