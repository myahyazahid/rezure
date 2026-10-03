import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { PostgresRelease, PostgresVersion } from '@/types/runtime'
import type { InstallProgress } from '@/types/binary'
import { useServicesStore } from '@/stores/services'
import { useDbConnectionsStore } from '@/stores/dbConnections'

// Keep in sync with `PROGRESS_EVENT` in src-tauri/src/services/binaries.rs
const PROGRESS_EVENT = 'binary://install-progress'

function errorMessage(e: unknown): string {
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  return 'Something went wrong.'
}

/**
 * PostgreSQL's versions: which are installed, which one the `postgres`
 * service runs, and the pinned list Rezure can install. Switching restarts a
 * running server on the new version — on that major's own data, see
 * `services::postgres`.
 */
export const usePostgresStore = defineStore('postgres', () => {
  const versions = ref<PostgresVersion[]>([])
  const switching = ref<string | null>(null)
  const error = ref<string | null>(null)

  const catalog = ref<PostgresRelease[]>([])
  const catalogLoading = ref(false)
  const catalogError = ref<string | null>(null)
  const installingVersion = ref<string | null>(null)

  const progress = ref<Record<string, InstallProgress>>({})
  listen<InstallProgress>(PROGRESS_EVENT, (event) => {
    progress.value[event.payload.id] = event.payload
  })

  const active = computed(() => versions.value.find((v) => v.active) ?? null)

  async function fetchVersions() {
    versions.value = await invoke<PostgresVersion[]>('list_postgres_versions')
  }

  /** Switches the active version. A running server is restarted on it, so
   *  the service card is re-read afterwards. */
  async function setActive(id: string) {
    error.value = null
    switching.value = id
    try {
      versions.value = await invoke<PostgresVersion[]>('set_active_postgres_version', { id })
      return true
    } catch (e) {
      error.value = errorMessage(e)
      await fetchVersions().catch(() => {})
      return false
    } finally {
      switching.value = null
      await useServicesStore()
        .fetchAll()
        .catch(() => {})
    }
  }

  /** A pinned list on the Rust side — nothing to fetch from the network. */
  async function fetchCatalog() {
    catalogLoading.value = true
    catalogError.value = null
    try {
      catalog.value = await invoke<PostgresRelease[]>('list_postgres_catalog')
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
      await invoke('install_postgres_version', { version })
      const entry = catalog.value.find((r) => r.version === version)
      if (entry) entry.installed = true
      await fetchVersions()
      // The service card flips to installed, and the first install brings
      // the PostgreSQL connection to the Databases switcher.
      await Promise.all([
        useServicesStore()
          .fetchAll()
          .catch(() => {}),
        useDbConnectionsStore()
          .fetchAll()
          .catch(() => {}),
      ])
      return true
    } catch (e) {
      catalogError.value = errorMessage(e)
      return false
    } finally {
      delete progress.value[`postgres-${version}`]
      installingVersion.value = null
    }
  }

  function progressFor(version: string) {
    return progress.value[`postgres-${version}`] ?? null
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
