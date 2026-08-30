import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type {
  DbProfileStatus,
  DetectedDatadir,
  SwitchResult,
} from '@/types/dbProfile'

function errorMessage(e: unknown): string {
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  return 'Something went wrong.'
}

export const useDbProfilesStore = defineStore('dbProfiles', () => {
  const profiles = ref<DbProfileStatus[]>([])
  const detected = ref<DetectedDatadir[]>([])
  const switchingId = ref<string | null>(null)
  const detecting = ref(false)
  const adding = ref(false)
  const error = ref<string | null>(null)
  const notice = ref<string | null>(null)

  const active = computed(() => profiles.value.find((p) => p.active) ?? null)

  async function fetchAll() {
    try {
      profiles.value = await invoke<DbProfileStatus[]>('list_db_profiles')
    } catch (e) {
      error.value = errorMessage(e)
    }
  }

  /** Scans for Laragon/XAMPP data. Read-only — nothing is registered until
   *  the user picks one. */
  async function detect() {
    detecting.value = true
    error.value = null
    try {
      detected.value = await invoke<DetectedDatadir[]>('detect_db_profiles')
    } catch (e) {
      error.value = errorMessage(e)
      detected.value = []
    } finally {
      detecting.value = false
    }
  }

  async function add(profile: {
    name: string
    datadirPath: string
    engine?: string | null
    version: string
    port: number
    source?: string | null
    binaryDir?: string | null
    defaultsFile?: string | null
  }) {
    adding.value = true
    error.value = null
    try {
      profiles.value = await invoke<DbProfileStatus[]>('add_db_profile', {
        request: {
          name: profile.name,
          datadirPath: profile.datadirPath,
          engine: profile.engine ?? null,
          version: profile.version,
          port: profile.port,
          source: profile.source ?? null,
          binaryDir: profile.binaryDir ?? null,
          defaultsFile: profile.defaultsFile ?? null,
        },
      })
      // Drop it from the detection list so a second scan isn't needed to
      // see it's been taken.
      detected.value = detected.value.filter(
        (d) => d.datadirPath.toLowerCase() !== profile.datadirPath.toLowerCase(),
      )
      return true
    } catch (e) {
      error.value = errorMessage(e)
      return false
    } finally {
      adding.value = false
    }
  }

  async function remove(id: string) {
    error.value = null
    try {
      profiles.value = await invoke<DbProfileStatus[]>('remove_db_profile', { id })
      return true
    } catch (e) {
      error.value = errorMessage(e)
      return false
    }
  }

  /**
   * Switches which datadir the server runs against. The backend gates the
   * switch before stopping anything and rolls back if the new profile won't
   * start, so a rejection here means nothing changed.
   */
  async function switchTo(id: string) {
    switchingId.value = id
    error.value = null
    notice.value = null
    try {
      const result = await invoke<SwitchResult>('switch_db_profile', { id })
      profiles.value = result.profiles
      const name = result.profiles.find((p) => p.id === id)?.name ?? 'that profile'
      notice.value = result.restarted
        ? `Now serving ${name}.`
        : `${name} is active — it'll be used the next time the database starts.`
      return true
    } catch (e) {
      error.value = errorMessage(e)
      return false
    } finally {
      switchingId.value = null
    }
  }

  return {
    profiles,
    detected,
    active,
    switchingId,
    detecting,
    adding,
    error,
    notice,
    fetchAll,
    detect,
    add,
    remove,
    switchTo,
  }
})
