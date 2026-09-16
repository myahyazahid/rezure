import { defineStore } from 'pinia'
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { NodeRelease, NodeVersion } from '@/types/runtime'
import type { InstallProgress } from '@/types/binary'

// Keep in sync with `PROGRESS_EVENT` in src-tauri/src/services/binaries.rs
const PROGRESS_EVENT = 'binary://install-progress'

function errorMessage(e: unknown): string {
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  return 'Something went wrong.'
}

/**
 * Catalog and install only — there's no "active version"/switch concept
 * here yet. Installing a version makes `bin/node/<version>/node.exe` exist
 * on disk; wiring it onto PATH or a project is a separate, later feature.
 */
export const useNodeStore = defineStore('node', () => {
  const versions = ref<NodeVersion[]>([])

  const catalog = ref<NodeRelease[]>([])
  const catalogLoading = ref(false)
  const catalogError = ref<string | null>(null)
  const installingVersion = ref<string | null>(null)

  const progress = ref<Record<string, InstallProgress>>({})
  listen<InstallProgress>(PROGRESS_EVENT, (event) => {
    progress.value[event.payload.id] = event.payload
  })

  async function fetchVersions() {
    versions.value = await invoke<NodeVersion[]>('list_node_versions')
  }

  async function fetchCatalog(refresh = false) {
    catalogLoading.value = true
    catalogError.value = null
    try {
      catalog.value = await invoke<NodeRelease[]>('list_node_catalog', { refresh })
    } catch (e) {
      catalogError.value = errorMessage(e)
      catalog.value = []
    } finally {
      catalogLoading.value = false
    }
  }

  async function installVersion(version: string) {
    installingVersion.value = version
    catalogError.value = null
    try {
      await invoke('install_node_version', { version })
      const entry = catalog.value.find((r) => r.version === version)
      if (entry) entry.installed = true
      await fetchVersions()
      return true
    } catch (e) {
      catalogError.value = errorMessage(e)
      return false
    } finally {
      delete progress.value[`node-${version}`]
      installingVersion.value = null
    }
  }

  function progressFor(version: string) {
    return progress.value[`node-${version}`] ?? null
  }

  return {
    versions,
    catalog,
    catalogLoading,
    catalogError,
    installingVersion,
    fetchVersions,
    fetchCatalog,
    installVersion,
    progressFor,
  }
})
