import { defineStore } from 'pinia'
import { ref, shallowRef } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { check, type Update } from '@tauri-apps/plugin-updater'
import type { UpgradeNotice } from '@/types/changelog'

function errorMessage(e: unknown): string {
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  return 'Something went wrong.'
}

export const useUpdateStore = defineStore('update', () => {
  const checking = ref(false)
  // `shallowRef`, not `ref`: `Update` is a class whose state lives in ES private
  // fields (`#rid`), and a deep `ref` hands out a reactive Proxy of it. Private
  // fields can't be read through a Proxy, so every method call on it threw
  // "Cannot read private member from an object whose class did not declare it"
  // and the Update button could never work. Found by scripts/test-updater.ps1.
  const available = shallowRef<Update | null>(null)
  const checkError = ref<string | null>(null)

  const downloading = ref(false)
  const downloadedBytes = ref(0)
  const totalBytes = ref<number | null>(null)
  const downloadError = ref<string | null>(null)

  const upgradeNotice = ref<UpgradeNotice | null>(null)

  /** The updater never crosses major lines (3.x only gets 3.x), so a newer
   *  major is announced separately. The command never fails — no notice
   *  and an unreachable API both come back as `null`. */
  async function fetchUpgradeNotice() {
    upgradeNotice.value = await invoke<UpgradeNotice | null>('fetch_upgrade_notice')
  }

  async function openUpgradeNotice() {
    if (!upgradeNotice.value) return
    await invoke('open_external_link', { url: upgradeNotice.value.url })
  }

  async function checkForUpdate() {
    checking.value = true
    checkError.value = null
    try {
      available.value = await check()
    } catch (e) {
      checkError.value = errorMessage(e)
    } finally {
      checking.value = false
    }
  }

  /** One click drives the whole sequence: download with progress, then
   *  install. On Windows `downloadAndInstall` exits the app itself once the
   *  installer has launched (it restarts the app after installing by
   *  default), so there's no separate relaunch step to call here. */
  async function downloadAndApply() {
    if (!available.value) return
    downloading.value = true
    downloadError.value = null
    downloadedBytes.value = 0
    totalBytes.value = null
    try {
      await available.value.downloadAndInstall((event) => {
        if (event.event === 'Started') totalBytes.value = event.data.contentLength ?? null
        else if (event.event === 'Progress') downloadedBytes.value += event.data.chunkLength
      })
    } catch (e) {
      downloadError.value = errorMessage(e)
    } finally {
      downloading.value = false
    }
  }

  return {
    checking,
    available,
    checkError,
    downloading,
    downloadedBytes,
    totalBytes,
    downloadError,
    upgradeNotice,
    checkForUpdate,
    downloadAndApply,
    fetchUpgradeNotice,
    openUpgradeNotice,
  }
})
