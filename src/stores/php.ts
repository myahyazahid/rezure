import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { PhpPathStatus, PhpRelease, PhpSwitchResult, PhpVersion } from '@/types/php'
import type { InstallProgress } from '@/types/binary'

// Keep in sync with `PROGRESS_EVENT` in src-tauri/src/services/binaries.rs
const PROGRESS_EVENT = 'binary://install-progress'

function errorMessage(e: unknown): string {
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  return 'Something went wrong.'
}

export const usePhpStore = defineStore('php', () => {
  const versions = ref<PhpVersion[]>([])
  const installingId = ref<string | null>(null)
  const error = ref<string | null>(null)

  const catalog = ref<PhpRelease[]>([])
  const catalogLoading = ref(false)
  const catalogError = ref<string | null>(null)

  const dropInDir = ref('')
  const adding = ref(false)

  /** Where a user's own php.ini fragments go. A label until they open it. */
  const configDir = ref('')

  /** The optional system-wide PATH link. Null until first read. */
  const pathStatus = ref<PhpPathStatus | null>(null)
  const pathBusy = ref(false)
  /** Short-lived confirmation shown after a switch, e.g. that the PHP
   *  service was reloaded onto the new version. */
  const notice = ref<string | null>(null)

  /** The version id currently being switched to, or null. A switch re-points
   *  the PATH junction and reloads the running PHP service, so it is slow
   *  enough to need the busy overlay. */
  const switching = ref<string | null>(null)

  /** Install progress, keyed by version — the backend emits the version as
   *  the progress id for a PHP install. */
  const progress = ref<Record<string, InstallProgress>>({})

  listen<InstallProgress>(PROGRESS_EVENT, (event) => {
    progress.value[event.payload.id] = event.payload
  })

  const active = computed(() => versions.value.find((v) => v.active) ?? null)

  async function fetchAll() {
    versions.value = await invoke<PhpVersion[]>('list_php_versions')
  }

  async function fetchDropInDir() {
    try {
      dropInDir.value = await invoke<string>('php_drop_in_dir')
    } catch {
      // Only used as a label — a missing path isn't worth an error banner.
      dropInDir.value = ''
    }
  }

  /**
   * Switches the active version. The backend also restarts the PHP service
   * when it's running, so the change takes effect without a manual reload —
   * `restarted` says whether that happened, and `restartError` when it was
   * attempted and failed.
   */
  async function setActive(id: string) {
    error.value = null
    notice.value = null
    switching.value = id
    try {
      const result = await invoke<PhpSwitchResult>('set_active_php_version', { id })
      versions.value = result.versions
      // The backend re-points the PATH link on a switch; re-read it so
      // the card doesn't keep showing the previous target.
      if (pathStatus.value?.onPath) await fetchPathStatus()

      if (result.restartError) {
        // The version did switch — this is about the service that failed to
        // come back up, so it can't be reported as a failed switch.
        error.value = `Switched to PHP ${id}, but the service didn't restart: ${result.restartError}`
      } else if (result.restarted) {
        notice.value = `PHP ${id} is active — the service was reloaded.`
      } else {
        notice.value = `PHP ${id} is active — it'll be used the next time PHP starts.`
      }
      return result
    } catch (e) {
      error.value = errorMessage(e)
      return null
    } finally {
      switching.value = null
    }
  }

  async function fetchPathStatus() {
    try {
      pathStatus.value = await invoke<PhpPathStatus>('php_path_status')
    } catch (e) {
      // Reading PATH shouldn't be able to break the page it sits on.
      error.value = errorMessage(e)
    }
  }

  /**
   * Turns the system-wide PATH link on or off. This is the one action that
   * changes something outside Rezure, so it only ever runs from an explicit
   * click — never as a side effect of switching versions.
   */
  async function setPathLink(enabled: boolean) {
    pathBusy.value = true
    error.value = null
    notice.value = null
    try {
      pathStatus.value = await invoke<PhpPathStatus>(
        enabled ? 'enable_php_path' : 'disable_php_path',
      )
      // Careful with the wording here: *adding* the entry only reaches
      // terminals started afterwards, because an open shell holds a copy of
      // the environment from when it launched. It's re-pointing the junction
      // on a later switch that open terminals follow, since the entry is
      // already in their PATH by then.
      notice.value = enabled
        ? "Rezure's PHP is on your PATH. Open a new terminal to pick it up — ones already open keep the PATH they started with."
        : 'Removed from your PATH. Whatever was there before takes over again in new terminals.'
    } catch (e) {
      error.value = errorMessage(e)
    } finally {
      pathBusy.value = false
    }
  }

  /** php.net's published Windows builds. `refresh` re-fetches instead of
   *  reusing the copy the backend cached for this session. */
  async function fetchCatalog(refresh = false) {
    catalogLoading.value = true
    catalogError.value = null
    try {
      catalog.value = await invoke<PhpRelease[]>('list_php_catalog', { refresh })
    } catch (e) {
      catalogError.value = errorMessage(e)
      catalog.value = []
    } finally {
      catalogLoading.value = false
    }
  }

  async function install(version: string) {
    installingId.value = version
    catalogError.value = null
    error.value = null
    try {
      versions.value = await invoke<PhpVersion[]>('install_php_version', { version })
      // Flip the just-installed entry over without a second network call.
      const entry = catalog.value.find((release) => release.version === version)
      if (entry) entry.installed = true
    } catch (e) {
      catalogError.value = errorMessage(e)
    } finally {
      delete progress.value[version]
      installingId.value = null
    }
  }

  /** Copies a PHP build the user already downloaded into the drop-in folder. */
  async function addFromFolder(path: string) {
    adding.value = true
    catalogError.value = null
    try {
      versions.value = await invoke<PhpVersion[]>('add_php_from_folder', { path })
      return true
    } catch (e) {
      catalogError.value = errorMessage(e)
      return false
    } finally {
      adding.value = false
    }
  }

  async function remove(version: string) {
    error.value = null
    try {
      versions.value = await invoke<PhpVersion[]>('remove_php_version', { version })
      const entry = catalog.value.find((release) => release.version === version)
      if (entry) entry.installed = false
    } catch (e) {
      error.value = errorMessage(e)
    }
  }

  async function fetchConfigDir() {
    try {
      configDir.value = await invoke<string>('php_config_dir')
    } catch {
      // Only used as a label — a missing path isn't worth an error banner.
      configDir.value = ''
    }
  }

  async function openConfigDir() {
    error.value = null
    try {
      await invoke('open_php_config_dir')
    } catch (e) {
      error.value = errorMessage(e)
    }
  }

  async function openDropInDir() {
    error.value = null
    try {
      await invoke('open_php_drop_in_dir')
    } catch (e) {
      error.value = errorMessage(e)
    }
  }

  function progressFor(version: string) {
    return progress.value[version] ?? null
  }

  return {
    versions,
    active,
    installingId,
    error,
    catalog,
    catalogLoading,
    catalogError,
    dropInDir,
    adding,
    configDir,
    notice,
    switching,
    pathStatus,
    pathBusy,
    fetchAll,
    fetchDropInDir,
    fetchConfigDir,
    fetchPathStatus,
    setPathLink,
    fetchCatalog,
    setActive,
    install,
    addFromFolder,
    remove,
    openDropInDir,
    openConfigDir,
    progressFor,
  }
})
