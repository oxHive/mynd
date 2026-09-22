<script setup>
import { ref, computed, onMounted, onBeforeUnmount } from 'vue'
import MarkdownContent from '../shared/MarkdownContent.vue'
import { useUpdateStore } from '../../stores/update.js'
import { getStatus } from '../../api/memories.js'

const update = useUpdateStore()
const emit = defineEmits(['close'])

const modalRef = ref(null)
const elapsed = ref(0)
const timedOut = ref(false)
const UPDATE_TIMEOUT_SECONDS = 180

let elapsedTimer = null
let pollTimer = null
let versionBeforeUpdate = null

const isUpdating = computed(() => update.status === 'updating')
const canDismiss = computed(() => !isUpdating.value)

function trapFocus(e) {
  if (!modalRef.value) return
  if (e.key === 'Escape') {
    if (canDismiss.value) emit('close')
    return
  }
  const focusable = modalRef.value.querySelectorAll('button, [href], input, [tabindex]:not([tabindex="-1"])')
  const first = focusable[0]
  const last = focusable[focusable.length - 1]
  if (e.key === 'Tab' && first && last) {
    if (e.shiftKey) {
      if (document.activeElement === first) { e.preventDefault(); last.focus() }
    } else {
      if (document.activeElement === last) { e.preventDefault(); first.focus() }
    }
  }
}

function startElapsedTimer() {
  stopElapsedTimer()
  elapsedTimer = setInterval(() => {
    if (!update.updateStartedAt) return
    elapsed.value = Math.max(0, Math.floor(Date.now() / 1000 - update.updateStartedAt))
    if (elapsed.value > UPDATE_TIMEOUT_SECONDS) timedOut.value = true
  }, 1000)
}
function stopElapsedTimer() {
  if (elapsedTimer) clearInterval(elapsedTimer)
  elapsedTimer = null
}

function startVersionPoll() {
  stopVersionPoll()
  pollTimer = setInterval(async () => {
    try {
      const data = await getStatus()
      const version = (data.info ?? data)?.version
      if (version && versionBeforeUpdate && version !== versionBeforeUpdate) {
        window.location.reload()
      }
    } catch {
      // server is between the old process exiting and the new one rebinding
      // the port — expected mid-restart, just keep polling
    }
  }, 2000)
}
function stopVersionPoll() {
  if (pollTimer) clearInterval(pollTimer)
  pollTimer = null
}

async function onUpdate() {
  timedOut.value = false
  elapsed.value = 0
  versionBeforeUpdate = update.currentVersion
  await update.startUpdate()
  startElapsedTimer()
  startVersionPoll()
}

function onReloadClick() {
  window.location.reload()
}

onMounted(() => {
  document.addEventListener('keydown', trapFocus)
  if (isUpdating.value) {
    versionBeforeUpdate = update.currentVersion
    startElapsedTimer()
    startVersionPoll()
  }
})
onBeforeUnmount(() => {
  document.removeEventListener('keydown', trapFocus)
  stopElapsedTimer()
  stopVersionPoll()
})
</script>

<template>
  <div class="fixed inset-0 z-40 flex items-center justify-center"
    style="background:rgba(0,0,0,0.6)"
    @click.self="canDismiss && emit('close')">
    <div ref="modalRef"
      role="dialog"
      aria-modal="true"
      aria-labelledby="update-modal-title"
      class="rounded-lg p-6 w-[28rem]"
      style="background:var(--hm-bg-overlay); border:0.5px solid var(--hm-border-default)">

      <template v-if="!isUpdating">
        <h3 id="update-modal-title" class="text-base font-medium mb-1" style="color:var(--hm-text-primary)">
          Update available: v{{ update.latestVersion }}
        </h3>
        <p class="text-xs mb-4" style="color:var(--hm-text-tertiary)">
          Current version: v{{ update.currentVersion }}
        </p>

        <div v-if="update.status === 'failed'"
          class="text-sm mb-4 p-3 rounded"
          style="background:var(--hm-danger-bg); border:0.5px solid var(--hm-danger-border); color:var(--hm-danger)">
          {{ update.error }}
        </div>

        <div class="max-h-72 overflow-y-auto mb-5 pr-1">
          <MarkdownContent :text="update.releaseNotesMd" />
        </div>

        <div class="flex justify-end gap-2">
          <button class="hm-btn hm-btn-default" @click="emit('close')">Later</button>
          <button class="hm-btn hm-btn-primary" @click="onUpdate">
            {{ update.status === 'failed' ? 'Retry update' : 'Update' }}
          </button>
        </div>
      </template>

      <template v-else>
        <h3 id="update-modal-title" class="text-base font-medium mb-4" style="color:var(--hm-text-primary)">
          Updating to v{{ update.latestVersion }}&hellip;
        </h3>

        <div v-if="!timedOut" class="flex flex-col items-center gap-3 py-6">
          <div class="hm-spinner" aria-hidden="true"></div>
          <p class="text-sm" style="color:var(--hm-text-secondary)">
            Elapsed: {{ elapsed }}s
          </p>
          <p class="text-xs" style="color:var(--hm-text-tertiary)">
            The server is restarting itself. This page will reload automatically.
          </p>
        </div>

        <div v-else class="py-4">
          <p class="text-sm mb-4" style="color:var(--hm-text-secondary)">
            Update is taking longer than expected (over {{ UPDATE_TIMEOUT_SECONDS }}s).
            Check the server logs, or reload to check its current state.
          </p>
          <div class="flex justify-end">
            <button class="hm-btn hm-btn-primary" @click="onReloadClick">Reload page</button>
          </div>
        </div>
      </template>
    </div>
  </div>
</template>

<style scoped>
.hm-spinner {
  width: 28px;
  height: 28px;
  border-radius: 50%;
  border: 3px solid var(--hm-border-default);
  border-top-color: var(--hm-accent);
  animation: hm-spin 0.8s linear infinite;
}
@keyframes hm-spin {
  to { transform: rotate(360deg); }
}
</style>
