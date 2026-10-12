<script setup>
import { ref, computed, watch, onMounted, onBeforeUnmount, nextTick } from 'vue'
import QRCode from 'qrcode'
import { useHiveStore } from '../../stores/hive.js'
import { useUiStore } from '../../stores/ui.js'
import { setHiveEnabled, issuePairingCode, addTrustedNetwork, removeTrustedNetwork } from '../../api/hive.js'
import CopyIdButton from '../shared/CopyIdButton.vue'
import HiveRevokeModal from './HiveRevokeModal.vue'
import SettingRow from './SettingRow.vue'
import Switch from '../shared/Switch.vue'

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

// Local switch state: flips optimistically on click, snaps back on error,
// and follows the store whenever hive status is refetched.
const enabled = ref(hive.enabled)
watch(() => hive.enabled, v => { enabled.value = v })

async function onToggleEnabled(next) {
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
      enabled.value = !next
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
  <p v-if="loading" class="text-[13px] text-fg-faint">Loading…</p>

  <template v-else>
    <p v-if="hive.loadError === 'forbidden'" class="text-[13px] text-fg-faint">
      Hive controls are only available from the machine running Mynd.
    </p>
    <template v-else>
      <div class="rounded-xl border border-line bg-card px-[18px] py-1">
        <SettingRow label="Hive sync" :hint="restarting ? 'Restarting the server to apply this change…' : undefined">
          <Switch v-model="enabled" label="Enable hive sync" :disabled="restarting" @update:model-value="onToggleEnabled" />
          <span v-if="toggleError" class="ml-3 text-xs text-danger">{{ toggleError }}</span>
        </SettingRow>

        <template v-if="hive.enabled && hive.identity">
          <hr class="m-0 h-px border-0 bg-line" />
          <SettingRow label="This device">
            <div class="flex min-w-0 items-center gap-2">
              <span class="font-headline font-semibold text-fg">{{ hive.identity.name }}</span>
              <span class="font-headline text-xs tabular-nums text-fg-faint" :title="hive.identity.public_key">{{ fingerprint }}</span>
              <CopyIdButton :id="hive.identity.public_key" />
            </div>
          </SettingRow>
          <hr class="m-0 h-px border-0 bg-line" />
          <SettingRow label="Invite" hint="Pair another device with this hive.">
            <button v-if="!pairing" type="button" class="hm-btn hm-btn-default hm-btn-sm" :disabled="pairingLoading" @click="showPairingCode">
              <span class="material-symbols-outlined text-[16px]!" aria-hidden="true">qr_code_2</span>{{ pairingLoading ? 'Issuing…' : 'Show pairing code' }}
            </button>
            <div v-else class="flex flex-wrap items-center gap-4">
              <canvas ref="qrCanvas" class="rounded-lg border border-line"></canvas>
              <div class="flex flex-col gap-1.5">
                <span class="hm-label mb-0">Code</span>
                <span class="font-headline text-lg font-bold tracking-[0.04em] tabular-nums text-fg-strong">{{ pairing.code }}</span>
                <span class="text-xs text-fg-faint">Expires {{ expiresIn(pairing.expires_at) }}</span>
              </div>
            </div>
          </SettingRow>
        </template>
      </div>

      <template v-if="hive.enabled && hive.identity">
        <div class="flex flex-col gap-2">
          <div class="flex items-baseline gap-2.5 px-0.5">
            <span class="hm-label mb-0">Members</span>
            <span v-if="hive.pendingConflictCount" class="text-xs font-bold text-gold">
              {{ hive.pendingConflictCount }} pending conflict{{ hive.pendingConflictCount === 1 ? '' : 's' }}
            </span>
          </div>
          <div class="divide-y divide-line overflow-hidden rounded-xl border border-line bg-card">
            <div v-for="peer in hive.roster" :key="peer.device_id" class="flex items-center gap-3 py-2.5 pl-[18px] pr-3.5">
              <span class="h-[7px] w-[7px] shrink-0 rounded-circle" :class="peer.online ? 'bg-ok-dot' : 'bg-line-strong'"
                :title="peer.online ? 'Online' : 'Offline'"></span>
              <span class="min-w-0 flex-1 truncate font-headline text-[13.5px] font-semibold text-fg">
                {{ peer.name }}
                <span v-if="peer.is_self" class="font-body text-xs font-normal text-fg-faint">(this device)</span>
              </span>
              <span class="w-20 text-right text-xs text-fg-faint">{{ peer.is_self ? '' : relativeTime(peer.last_synced_at) }}</span>
              <!-- A device can't revoke itself (the API refuses it too): that
                   would lock it out of its own hive with no way back. -->
              <button v-if="peer.status !== 'revoked' && !peer.is_self" type="button"
                class="hm-btn hm-btn-ghost hm-btn-sm text-danger hover:bg-danger-soft" @click="askRevoke(peer)">Revoke</button>
              <span v-else-if="peer.status === 'revoked'" class="text-xs text-fg-faint">Revoked</span>
            </div>
            <p v-if="!hive.roster.length" class="m-0 py-3.5 pl-[18px] text-[13px] text-fg-faint">No other devices in this hive yet.</p>
          </div>
        </div>

        <div class="flex flex-col gap-2">
          <div class="flex items-baseline gap-2.5 px-0.5">
            <span class="hm-label mb-0">Trusted networks</span>
            <span class="text-xs text-fg-faint">Hive sync auto-pauses on networks not in this list.</span>
          </div>
          <div class="divide-y divide-line overflow-hidden rounded-xl border border-line bg-card">
            <div v-for="net in hive.trustedNetworks" :key="net.id" class="flex items-center gap-3 py-2 pl-[18px] pr-3.5">
              <span class="min-w-0 flex-1 truncate font-headline text-[13px] text-fg">{{ net.label || net.id }}</span>
              <button type="button" class="hm-btn hm-btn-ghost hm-btn-sm" @click="removeTrusted(net.id)">Remove</button>
            </div>
            <div v-if="hive.currentNetwork && !hive.trustedNetworks.some(n => n.id === hive.currentNetwork)"
              class="flex items-center gap-2.5 bg-gold-soft py-2.5 pl-[18px] pr-3.5">
              <span class="mr-auto text-[12.5px] text-fg-muted">Current network not trusted.</span>
              <button type="button" class="hm-btn hm-btn-default hm-btn-sm" @click="trustCurrentNetwork">Trust current network</button>
            </div>
            <div class="flex flex-wrap items-center gap-2.5 py-3 pl-[18px] pr-3.5">
              <input v-model="newTrustedId" class="hm-input w-[160px] font-headline" placeholder="network id" aria-label="Network id" @keydown.enter="addTrusted" />
              <input v-model="newTrustedLabel" class="hm-input w-[160px]" placeholder="label (optional)" aria-label="Network label" @keydown.enter="addTrusted" />
              <button type="button" class="hm-btn hm-btn-default hm-btn-sm" :disabled="!newTrustedId.trim()" @click="addTrusted">
                <span class="material-symbols-outlined text-[16px]!" aria-hidden="true">add</span>Add
              </button>
            </div>
          </div>
          <p v-if="trustedError" class="m-0 text-xs text-danger">{{ trustedError }}</p>
        </div>
      </template>
    </template>
  </template>

  <HiveRevokeModal v-if="revokeTarget" :device-id="revokeTarget.device_id" :device-name="revokeTarget.name"
    @close="revokeTarget = null" />
</template>
