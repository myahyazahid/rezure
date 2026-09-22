import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
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
 * Catalog, install, and the globally active version. Installing makes
 * `bin/node/<version>/node.exe` exist on disk; switching `active` changes
 * what `services::launcher::open_terminal` puts first on a new terminal's
 * PATH for a project that hasn't pinned its own version (see
 * `Project.nodeVersion` / `useProjectsStore().setNodeVersion`). There's no
 * system-wide "Node Everywhere" PATH link yet (unlike PHP's) — the active
 * version only reaches terminals Rezure itself opens.
 */
export const useNodeStore = defineStore('node', () => {
  const versions = ref<NodeVersion[]>([])
  const switching = ref<string | null>(null)
  const error = ref<string | null>(null)

  const catalog = ref<NodeRelease[]>([])
  const catalogLoading = ref(false)
  const catalogError = ref<string | null>(null)
  const installingVersion = ref<string | null>(null)

  const progress = ref<Record<string, InstallProgress>>({})
  listen<InstallProgress>(PROGRESS_EVENT, (event) => {
    progress.value[event.payload.id] = event.payload
  })

  const active = computed(() => versions.value.find((v) => v.active) ?? null)

  async function fetchVersions() {
    versions.value = await invoke<NodeVersion[]>('list_node_versions')
  }

  /** Switches the global active version. */
  async function setActive(id: string) {
    error.value = null
    switching.value = id
    try {
      versions.value = await invoke<NodeVersion[]>('set_active_node_version', { id })
      return true
    } catch (e) {
      error.value = errorMessage(e)
      return false
    } finally {
      switching.value = null
    }
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
    active,
    switching,
    error,
    catalog,
    catalogLoading,
    catalogError,
    installingVersion,
    fetchVersions,
    setActive,
    fetchCatalog,
    installVersion,
    progressFor,
  }
})
