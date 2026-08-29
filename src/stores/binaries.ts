import { defineStore } from 'pinia'
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { BinaryStatus, InstallProgress } from '@/types/binary'

// Keep in sync with `PROGRESS_EVENT` in src-tauri/src/services/binaries.rs
const PROGRESS_EVENT = 'binary://install-progress'

export const useBinariesStore = defineStore('binaries', () => {
  const binaries = ref<BinaryStatus[]>([])
  const loading = ref(false)
  const installingIds = ref<Set<string>>(new Set())
  const progress = ref<Record<string, InstallProgress>>({})

  listen<InstallProgress>(PROGRESS_EVENT, (event) => {
    progress.value[event.payload.id] = event.payload
  })

  async function fetchAll() {
    loading.value = true
    try {
      binaries.value = await invoke<BinaryStatus[]>('list_binaries')
    } finally {
      loading.value = false
    }
  }

  async function install(id: string) {
    installingIds.value.add(id)
    try {
      const updated = await invoke<BinaryStatus>('install_binary', { id })
      const index = binaries.value.findIndex((b) => b.id === id)
      if (index !== -1) binaries.value[index] = updated
    } finally {
      installingIds.value.delete(id)
      delete progress.value[id]
    }
  }

  function isInstalling(id: string) {
    return installingIds.value.has(id)
  }

  function progressFor(id: string) {
    return progress.value[id] ?? null
  }

  return {
    binaries,
    loading,
    fetchAll,
    install,
    isInstalling,
    progressFor,
  }
})
