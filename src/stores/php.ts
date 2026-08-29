import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { PhpVersion } from '@/types/php'

function errorMessage(e: unknown): string {
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  return 'Something went wrong.'
}

export const usePhpStore = defineStore('php', () => {
  const versions = ref<PhpVersion[]>([])
  const installingId = ref<string | null>(null)
  const error = ref<string | null>(null)

  const active = computed(() => versions.value.find((v) => v.active) ?? null)

  async function fetchAll() {
    versions.value = await invoke<PhpVersion[]>('list_php_versions')
  }

  async function setActive(id: string) {
    error.value = null
    try {
      versions.value = await invoke<PhpVersion[]>('set_active_php_version', { id })
    } catch (e) {
      error.value = errorMessage(e)
    }
  }

  /** Downloads a not-yet-installed PHP version (a `binaries::MANIFEST` entry). */
  async function install(id: string) {
    installingId.value = id
    error.value = null
    try {
      await invoke('install_binary', { id })
      await fetchAll()
    } catch (e) {
      error.value = errorMessage(e)
    } finally {
      installingId.value = null
    }
  }

  return { versions, active, installingId, error, fetchAll, setActive, install }
})
