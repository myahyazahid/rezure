import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { ComposerRelease, ComposerVersion } from '@/types/runtime'
import type { InstallProgress } from '@/types/binary'

// Keep in sync with `PROGRESS_EVENT` in src-tauri/src/services/binaries.rs
const PROGRESS_EVENT = 'binary://install-progress'

function errorMessage(e: unknown): string {
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  return 'Something went wrong.'
}

export const useComposerStore = defineStore('composer', () => {
  /** Kept for the pre-existing "is Composer ready at all" check (the
   *  requirements-check flow) — now backed by `services::composer`'s
   *  version-aware storage rather than a single fixed path. */
  const installed = ref(false)
  const installing = ref(false)
  const error = ref<string | null>(null)

  /** Versions currently on disk, and which one is active. */
  const versions = ref<ComposerVersion[]>([])
  const active = computed(() => versions.value.find((v) => v.active) ?? null)
  const switching = ref<string | null>(null)

  /** Stable releases Composer currently publishes. */
  const catalog = ref<ComposerRelease[]>([])
  const catalogLoading = ref(false)
  const catalogError = ref<string | null>(null)
  const installingVersion = ref<string | null>(null)

  const progress = ref<Record<string, InstallProgress>>({})
  listen<InstallProgress>(PROGRESS_EVENT, (event) => {
    progress.value[event.payload.id] = event.payload
  })

  async function fetchStatus() {
    installed.value = await invoke<boolean>('composer_installed')
  }

  /** Downloads the newest stable release if nothing is installed yet — the
   *  pre-existing entry point, kept for the requirements-check flow. */
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

  async function fetchVersions() {
    versions.value = await invoke<ComposerVersion[]>('list_composer_versions')
  }

  async function setActive(id: string) {
    error.value = null
    switching.value = id
    try {
      versions.value = await invoke<ComposerVersion[]>('set_active_composer_version', { id })
    } catch (e) {
      error.value = errorMessage(e)
    } finally {
      switching.value = null
    }
  }

  async function fetchCatalog(refresh = false) {
    catalogLoading.value = true
    catalogError.value = null
    try {
      catalog.value = await invoke<ComposerRelease[]>('list_composer_catalog', { refresh })
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
      versions.value = await invoke<ComposerVersion[]>('install_composer_version', { version })
      installed.value = true
      const entry = catalog.value.find((r) => r.version === version)
      if (entry) entry.installed = true
      return true
    } catch (e) {
      catalogError.value = errorMessage(e)
      return false
    } finally {
      delete progress.value[`composer-${version}`]
      installingVersion.value = null
    }
  }

  function progressFor(version: string) {
    return progress.value[`composer-${version}`] ?? null
  }

  return {
    installed,
    installing,
    error,
    versions,
    active,
    switching,
    catalog,
    catalogLoading,
    catalogError,
    installingVersion,
    fetchStatus,
    install,
    fetchVersions,
    setActive,
    fetchCatalog,
    installVersion,
    progressFor,
  }
})
