import { defineStore } from 'pinia'
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { BinaryStatus, InstallProgress } from '@/types/binary'
import type { InstalledVersion, MariaDbRelease } from '@/types/runtime'

// Keep in sync with `PROGRESS_EVENT` in src-tauri/src/services/binaries.rs
const PROGRESS_EVENT = 'binary://install-progress'

function errorMessage(e: unknown): string {
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  return 'Something went wrong.'
}

export const useBinariesStore = defineStore('binaries', () => {
  const binaries = ref<BinaryStatus[]>([])
  const loading = ref(false)
  const installingIds = ref<Set<string>>(new Set())
  const progress = ref<Record<string, InstallProgress>>({})

  /** MariaDB versions currently on disk — the Switch page's dropdown for
   *  MariaDB reads this, not `binaries` (which only ever has the one
   *  pinned manifest entry). */
  const mariadbVersions = ref<InstalledVersion[]>([])

  /** MariaDB versions across every branch `services::mariadb_catalog` asks
   *  about — fetched separately from `binaries` since it hits the network
   *  live rather than reading the pinned manifest. */
  const mariadbCatalog = ref<MariaDbRelease[]>([])
  const mariadbCatalogLoading = ref(false)
  const mariadbCatalogError = ref<string | null>(null)
  const installingMariaDbVersion = ref<string | null>(null)

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

  async function fetchMariaDbVersions() {
    mariadbVersions.value = await invoke<InstalledVersion[]>('list_mariadb_versions')
  }

  /** MariaDB's release index, across every branch Rezure asks about.
   *  `refresh` re-fetches instead of reusing the copy the backend cached. */
  async function fetchMariaDbCatalog(refresh = false) {
    mariadbCatalogLoading.value = true
    mariadbCatalogError.value = null
    try {
      mariadbCatalog.value = await invoke<MariaDbRelease[]>('list_mariadb_catalog', { refresh })
    } catch (e) {
      mariadbCatalogError.value = errorMessage(e)
      mariadbCatalog.value = []
    } finally {
      mariadbCatalogLoading.value = false
    }
  }

  async function installMariaDbVersion(version: string) {
    installingMariaDbVersion.value = version
    mariadbCatalogError.value = null
    try {
      await invoke('install_mariadb_version', { version })
      const entry = mariadbCatalog.value.find((r) => r.version === version)
      if (entry) entry.installed = true
      await fetchMariaDbVersions()
      return true
    } catch (e) {
      mariadbCatalogError.value = errorMessage(e)
      return false
    } finally {
      delete progress.value[`mariadb-${version}`]
      installingMariaDbVersion.value = null
    }
  }

  return {
    binaries,
    loading,
    fetchAll,
    install,
    isInstalling,
    progressFor,
    mariadbVersions,
    mariadbCatalog,
    mariadbCatalogLoading,
    mariadbCatalogError,
    installingMariaDbVersion,
    fetchMariaDbVersions,
    fetchMariaDbCatalog,
    installMariaDbVersion,
  }
})
