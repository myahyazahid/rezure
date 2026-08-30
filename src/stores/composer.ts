import { defineStore } from 'pinia'
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'

function errorMessage(e: unknown): string {
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  return 'Something went wrong.'
}

/**
 * Composer has no fixed version to switch between — `install_composer`
 * always fetches whatever's current at Composer's own stable download URL
 * (see `services::scaffold::ensure_composer`), so this only tracks
 * whether it's been downloaded at all.
 */
export const useComposerStore = defineStore('composer', () => {
  const installed = ref(false)
  const installing = ref(false)
  const error = ref<string | null>(null)

  async function fetchStatus() {
    installed.value = await invoke<boolean>('composer_installed')
  }

  async function install() {
    installing.value = true
    error.value = null
    try {
      await invoke('install_composer')
      installed.value = true
    } catch (e) {
      error.value = errorMessage(e)
    } finally {
      installing.value = false
    }
  }

  return { installed, installing, error, fetchStatus, install }
})
