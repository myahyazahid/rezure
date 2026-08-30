import { defineStore } from 'pinia'
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { Settings, SettingsPatch, StoragePaths } from '@/types/settings'

function errorMessage(e: unknown): string {
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  return 'Something went wrong.'
}

export const useSettingsStore = defineStore('settings', () => {
  const defaultPort = ref(80)
  const shareUsageData = ref(false)
  const activePhpVersion = ref<string | null>(null)

  const storagePaths = ref<StoragePaths | null>(null)
  const error = ref<string | null>(null)
  const saving = ref(false)

  function apply(settings: Settings) {
    defaultPort.value = settings.defaultPort
    shareUsageData.value = settings.shareUsageData
    activePhpVersion.value = settings.activePhpVersion
  }

  async function fetchAll() {
    try {
      apply(await invoke<Settings>('get_settings'))
    } catch (e) {
      error.value = errorMessage(e)
    }
  }

  async function fetchStoragePaths() {
    try {
      storagePaths.value = await invoke<StoragePaths>('storage_paths')
    } catch {
      // Informational only — a Settings page that can't show its "where
      // things live" panel shouldn't otherwise be unusable.
      storagePaths.value = null
    }
  }

  async function update(patch: SettingsPatch) {
    saving.value = true
    error.value = null
    try {
      apply(await invoke<Settings>('update_settings', { patch }))
      return true
    } catch (e) {
      error.value = errorMessage(e)
      return false
    } finally {
      saving.value = false
    }
  }

  const setDefaultPort = (port: number) => update({ defaultPort: port })
  const setShareUsageData = (share: boolean) => update({ shareUsageData: share })

  return {
    defaultPort,
    shareUsageData,
    activePhpVersion,
    storagePaths,
    error,
    saving,
    fetchAll,
    fetchStoragePaths,
    setDefaultPort,
    setShareUsageData,
  }
})
