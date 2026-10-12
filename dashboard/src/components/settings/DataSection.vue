<script setup>
import { ref } from 'vue'
import { useUiStore } from '../../stores/ui.js'
import { exportMemories, importMemories } from '../../api/memories.js'
import DataRow from './DataRow.vue'

const ui = useUiStore()
const fileInput = ref(null)

async function handleExport() {
  try {
    const data = await exportMemories()
    const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = `mynd-export-${new Date().toISOString().slice(0,10)}.json`
    a.click()
    URL.revokeObjectURL(url)
  } catch {
    ui.showToast('Export failed')
  }
}

async function handleImport(e) {
  const file = e.target.files[0]
  if (!file) return
  try {
    const text = await file.text()
    const result = await importMemories(JSON.parse(text))
    ui.showToast(`Import complete — ${result.imported ?? '?'} memories imported`)
  } catch {
    ui.showToast('Import failed — invalid file format')
  }
  fileInput.value.value = ''
}
</script>

<template>
  <div class="rounded-xl border border-line bg-card px-[18px] py-1">
    <DataRow title="Export all memories as JSON" desc="Download personal and workspace memories as a backup. Org-layer memories are not included.">
      <button type="button" class="hm-btn hm-btn-default" @click="handleExport">
        <span class="material-symbols-outlined text-[17px]!" aria-hidden="true">download</span>Export
      </button>
    </DataRow>
    <hr class="m-0 h-px border-0 bg-line" />
    <DataRow title="Import from JSON" desc="Restore personal and workspace memories from a previous Mynd export.">
      <button type="button" class="hm-btn hm-btn-default" @click="fileInput.click()">
        <span class="material-symbols-outlined text-[17px]!" aria-hidden="true">upload</span>Import
      </button>
      <input ref="fileInput" type="file" accept=".json" class="hidden" @change="handleImport" />
    </DataRow>
  </div>
</template>
