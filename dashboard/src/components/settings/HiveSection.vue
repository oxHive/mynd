<script setup>
import { ref, computed, onMounted, onBeforeUnmount, nextTick } from 'vue'
import QRCode from 'qrcode'
import { useHiveStore } from '../../stores/hive.js'
import { useUiStore } from '../../stores/ui.js'
import { setHiveEnabled, issuePairingCode, addTrustedNetwork, removeTrustedNetwork } from '../../api/hive.js'
import CopyIdButton from '../shared/CopyIdButton.vue'
import HiveRevokeModal from './HiveRevokeModal.vue'

const hive = useHiveStore()
const ui = useUiStore()

const loading = ref(true)
const restarting = ref(false)
const toggleError = ref('')

const pairing = ref(null) // { code, expires_at, public_key }
const pairingLoading = ref(false)
const qrCanvas = ref(null)

const revokeTarget = ref(null) // { device_id, name } | null

const newTrustedId = ref('')
const newTrustedLabel = ref('')
const trustedError = ref('')

let restartPollTimer = null

onMounted(async () => {
  loading.value = true
  await Promise.all([hive.fetchStatus(), hive.fetchTrustedNetworks()])
  hive.primeOnlineSnapshot()
  loading.value = false
})

onBeforeUnmount(() => {
  if (restartPollTimer) clearInterval(restartPollTimer)
})

async function onToggleEnabled(e) {
  const next = e.target.checked
  toggleError.value = ''
  try {
    const res = await setHiveEnabled(next)
    if (res.restarting) {
      restarting.value = true
      startRestartPoll()
    }
  } catch (err) {
    if (err.status === undefined) {
      // Network-level failure, not an HTTP error response -- the server may
      // have aborted its listeners (mid-restart) before the response could
      // flush. The DB override write happens before the restart signal, so
      // treat this as "probably restarting" rather than a real failure.
      restarting.value = true
      startRestartPoll()
    } else {
      toggleError.value = 'Could not change the hive toggle — server error.'
      e.target.checked = !next
    }
  }
}

function startRestartPoll() {
  if (restartPollTimer) clearInterval(restartPollTimer)
  restartPollTimer = setInterval(async () => {
    await ui.pollServerStatus()
    if (ui.serverStatus === 'running') {
      clearInterval(restartPollTimer)
      restartPollTimer = null
      restarting.value = false
      await Promise.all([hive.fetchStatus(), hive.fetchTrustedNetworks()])
      hive.primeOnlineSnapshot()
      ui.showToast(hive.enabled ? 'Hive enabled' : 'Hive disabled')
    }
  }, 1500)
}

const fingerprint = computed(() => {
  const key = hive.identity?.public_key
  return key ? key.slice(0, 16) + '…' : ''
})

async function showPairingCode() {
  pairingLoading.value = true
  try {
    pairing.value = await issuePairingCode()
    await nextTick()
    drawQr()
  } catch {
    ui.showToast('Could not issue a pairing code — server error.')
  } finally {
    pairingLoading.value = false
  }
}

function drawQr() {
  if (!qrCanvas.value || !pairing.value) return
  const payload = JSON.stringify({
    code: pairing.value.code,
    public_key: pairing.value.public_key,
  })
  QRCode.toCanvas(qrCanvas.value, payload, { width: 200 }, (err) => {
    if (err) ui.showToast('Could not render invite QR code.')
  })
}

function relativeTime(unixSeconds) {
  if (!unixSeconds) return 'never'
  const diffSec = Math.floor(Date.now() / 1000) - unixSeconds
  if (diffSec < 60) return 'just now'
  if (diffSec < 3600) return `${Math.floor(diffSec / 60)}m ago`
  if (diffSec < 86400) return `${Math.floor(diffSec / 3600)}h ago`
  return `${Math.floor(diffSec / 86400)}d ago`
}

// For a future timestamp (pairing code expiry), not past ones like
// last-synced-at — relativeTime() above counts the other direction.
function expiresIn(unixSeconds) {
  const diffSec = unixSeconds - Math.floor(Date.now() / 1000)
  if (diffSec <= 0) return 'expired'
  if (diffSec < 60) return `in ${diffSec}s`
  return `in ${Math.floor(diffSec / 60)}m`
}

function askRevoke(peer) {
  revokeTarget.value = { device_id: peer.device_id, name: peer.name }
}

async function trustCurrentNetwork() {
  if (!hive.currentNetwork) return
  await addAndRefresh(hive.currentNetwork, null)
}

async function addTrusted() {
  const id = newTrustedId.value.trim()
  if (!id) return
  await addAndRefresh(id, newTrustedLabel.value.trim() || null)
  newTrustedId.value = ''
  newTrustedLabel.value = ''
}

async function addAndRefresh(id, label) {
  trustedError.value = ''
  try {
    await addTrustedNetwork(id, label)
    await hive.fetchTrustedNetworks()
  } catch {
    trustedError.value = 'Could not add that network — server error.'
  }
}

async function removeTrusted(id) {
  trustedError.value = ''
  try {
    await removeTrustedNetwork(id)
    await hive.fetchTrustedNetworks()
  } catch {
    trustedError.value = 'Could not remove that network — server error.'
  }
}
</script>

<template>
  <div>
    <p class="hm-label mb-4">HIVE</p>
    <p v-if="loading" style="font-size:13px; color:var(--hm-text-tertiary)">Loading…</p>

    <template v-else>
      <p v-if="hive.loadError === 'forbidden'" style="font-size:12px; color:var(--hm-text-tertiary)">
        Hive controls are only available from the machine running HiveMind.
      </p>
      <template v-else>
      <label class="flex items-center gap-3 mb-2 cursor-pointer">
        <input type="checkbox" class="w-4 h-4" :checked="hive.enabled" :disabled="restarting" @change="onToggleEnabled" />
        <span style="font-size:13px; color:var(--hm-text-primary)">Enable hive sync</span>
      </label>
      <p v-if="restarting" class="mb-5" style="font-size:12px; color:var(--hm-text-tertiary)">
        Restarting the server to apply this change…
      </p>
      <p v-else-if="toggleError" class="mb-5" style="font-size:12px; color:var(--hm-danger)">{{ toggleError }}</p>
      <div v-else class="mb-5"></div>

      <template v-if="hive.enabled && hive.identity">
        <!-- Identity -->
        <p class="hm-label" style="margin-bottom:8px">IDENTITY</p>
        <div class="flex items-center gap-2 mb-5">
          <span style="font-size:13px; color:var(--hm-text-primary)">{{ hive.identity.name }}</span>
          <span class="font-mono" style="font-size:11px; color:var(--hm-text-tertiary)" :title="hive.identity.public_key">
            {{ fingerprint }}
          </span>
          <CopyIdButton :id="hive.identity.public_key" />
        </div>

        <!-- Invite -->
        <p class="hm-label" style="margin-bottom:8px">INVITE</p>
        <button v-if="!pairing" class="hm-btn hm-btn-default hm-btn-sm mb-5" :disabled="pairingLoading" @click="showPairingCode">
          {{ pairingLoading ? 'Issuing…' : 'Show pairing code' }}
        </button>
        <div v-else class="mb-5 p-3 rounded-md flex items-center gap-4"
          style="background:var(--hm-bg-elevated); border:0.5px solid var(--hm-border-subtle)">
          <canvas ref="qrCanvas"></canvas>
          <div>
            <p class="hm-label" style="margin-bottom:4px">CODE</p>
            <p class="font-mono mb-2" style="font-size:16px; color:var(--hm-text-primary)">{{ pairing.code }}</p>
            <p style="font-size:11px; color:var(--hm-text-tertiary)">
              Expires {{ expiresIn(pairing.expires_at) }}
            </p>
          </div>
        </div>

        <!-- Members -->
        <p class="hm-label" style="margin-bottom:8px">
          MEMBERS
          <span v-if="hive.pendingConflictCount" style="color:var(--hm-warning); text-transform:none; letter-spacing:0">
            &middot; {{ hive.pendingConflictCount }} pending conflict{{ hive.pendingConflictCount === 1 ? '' : 's' }}
          </span>
        </p>
        <div v-for="peer in hive.roster" :key="peer.device_id" class="flex items-center gap-3 py-2"
          style="border-bottom:0.5px solid var(--hm-border-subtle)">
          <span class="rounded-full" style="width:8px; height:8px; flex-shrink:0"
            :style="{ background: peer.online ? 'var(--hm-success)' : 'var(--hm-text-tertiary)' }"
            :title="peer.online ? 'Online' : 'Offline'"></span>
          <span class="flex-1" style="font-size:13px; color:var(--hm-text-primary)">
            {{ peer.name }}
            <span v-if="peer.is_self" style="font-size:11px; color:var(--hm-text-tertiary)">(this device)</span>
          </span>
          <span style="font-size:11px; color:var(--hm-text-tertiary); width:80px; text-align:right">
            {{ peer.is_self ? '' : relativeTime(peer.last_synced_at) }}
          </span>
          <!-- A device can't revoke itself (the API refuses it too): that
               would lock it out of its own hive with no way back. -->
          <button v-if="peer.status !== 'revoked' && !peer.is_self" class="hm-btn hm-btn-ghost hm-btn-sm"
            style="color:var(--hm-danger)" @click="askRevoke(peer)">Revoke</button>
          <span v-else-if="peer.status === 'revoked'" style="font-size:11px; color:var(--hm-text-tertiary)">Revoked</span>
        </div>
        <p v-if="!hive.roster.length" class="mb-5" style="font-size:12px; color:var(--hm-text-tertiary)">
          No other devices in this hive yet.
        </p>
        <div class="mb-8"></div>

        <!-- Trusted networks -->
        <p class="hm-label" style="margin-bottom:8px">TRUSTED NETWORKS</p>
        <p style="font-size:11px; color:var(--hm-text-tertiary)" class="mb-3">
          Hive sync auto-pauses on networks not in this list.
        </p>
        <div v-for="net in hive.trustedNetworks" :key="net.id" class="flex items-center gap-3 py-2"
          style="border-bottom:0.5px solid var(--hm-border-subtle)">
          <span class="font-mono flex-1" style="font-size:12px; color:var(--hm-text-primary)">{{ net.label || net.id }}</span>
          <button class="hm-btn hm-btn-ghost hm-btn-sm" style="color:var(--hm-text-tertiary)" @click="removeTrusted(net.id)">Remove</button>
        </div>
        <div v-if="hive.currentNetwork && !hive.trustedNetworks.some(n => n.id === hive.currentNetwork)"
          class="flex items-center gap-2 mt-3 mb-3">
          <span style="font-size:12px; color:var(--hm-text-secondary)">Current network not trusted.</span>
          <button class="hm-btn hm-btn-default hm-btn-sm" @click="trustCurrentNetwork">Trust current network</button>
        </div>
        <div class="flex items-center gap-2 mt-3 rounded-md p-3" style="border:1px dashed var(--hm-border-default)">
          <input class="hm-input" style="width:160px" v-model="newTrustedId" placeholder="network id" />
          <input class="hm-input" style="width:160px" v-model="newTrustedLabel" placeholder="label (optional)" />
          <button class="hm-btn hm-btn-default hm-btn-sm" @click="addTrusted">+ Add</button>
        </div>
        <p v-if="trustedError" style="font-size:11px; color:var(--hm-danger)" class="mt-2">{{ trustedError }}</p>
      </template>
      </template>
    </template>

    <HiveRevokeModal v-if="revokeTarget" :device-id="revokeTarget.device_id" :device-name="revokeTarget.name"
      @close="revokeTarget = null" />
  </div>
</template>
