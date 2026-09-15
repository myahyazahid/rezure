import { defineStore } from 'pinia'
import { ref } from 'vue'
import { check, type Update } from '@tauri-apps/plugin-updater'

function errorMessage(e: unknown): string {
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  return 'Something went wrong.'
}

export const useUpdateStore = defineStore('update', () => {
  const checking = ref(false)
  const available = ref<Update | null>(null)
  const checkError = ref<string | null>(null)

  const downloading = ref(false)
  const downloadedBytes = ref(0)
  const totalBytes = ref<number | null>(null)
  const downloadError = ref<string | null>(null)

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
    checkForUpdate,
    downloadAndApply,
  }
})
