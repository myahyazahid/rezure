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
  const startWithWindows = ref(false)
  const keepInTrayOnClose = ref(false)
  const notifyOnCrash = ref(false)
  const domainSuffix = ref('test')
  const autoWriteHosts = ref(false)

  const storagePaths = ref<StoragePaths | null>(null)
  const error = ref<string | null>(null)
  const saving = ref(false)

  function apply(settings: Settings) {
    defaultPort.value = settings.defaultPort
    shareUsageData.value = settings.shareUsageData
    activePhpVersion.value = settings.activePhpVersion
    startWithWindows.value = settings.startWithWindows
    keepInTrayOnClose.value = settings.keepInTrayOnClose
    notifyOnCrash.value = settings.notifyOnCrash
    domainSuffix.value = settings.domainSuffix
    autoWriteHosts.value = settings.autoWriteHosts
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
  const setStartWithWindows = (enabled: boolean) => update({ startWithWindows: enabled })
  const setKeepInTrayOnClose = (enabled: boolean) => update({ keepInTrayOnClose: enabled })
  const setNotifyOnCrash = (enabled: boolean) => update({ notifyOnCrash: enabled })
  const setDomainSuffix = (suffix: string) => update({ domainSuffix: suffix })
  const setAutoWriteHosts = (enabled: boolean) => update({ autoWriteHosts: enabled })

  return {
    defaultPort,
    shareUsageData,
    activePhpVersion,
    startWithWindows,
    keepInTrayOnClose,
    notifyOnCrash,
    domainSuffix,
    autoWriteHosts,
    storagePaths,
    error,
    saving,
    fetchAll,
    fetchStoragePaths,
    setDefaultPort,
    setShareUsageData,
    setStartWithWindows,
    setKeepInTrayOnClose,
    setNotifyOnCrash,
    setDomainSuffix,
    setAutoWriteHosts,
  }
})
