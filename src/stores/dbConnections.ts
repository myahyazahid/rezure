import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { DbConnectionStatus, TargetResult, TlsMode } from '@/types/dbConnection'
import type { DbEngine } from '@/types/dbProfile'

function errorMessage(e: unknown): string {
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  return 'Something went wrong.'
}

/** The form payload every connection command takes, matching the Rust
 *  `ConnectionRequest`. Kept in one place because "test" and "save" send
 *  exactly the same thing — testing something other than what gets saved
 *  would make a green test meaningless. */
export interface ConnectionDraft {
  name: string
  host: string
  port: number
  user: string
  password: string
  engine: DbEngine
  tlsMode: TlsMode
  readOnly: boolean
  savePassword: boolean
  useSsh: boolean
  sshHost: string
  sshPort: number
  sshUser: string
  sshAuth: 'key' | 'password'
  sshKeyPath: string
  sshPassword: string
}

export const useDbConnectionsStore = defineStore('dbConnections', () => {
  const connections = ref<DbConnectionStatus[]>([])
  const error = ref<string | null>(null)
  const notice = ref<string | null>(null)

  const testing = ref(false)
  /** The server's own version string on success — proof it really answered,
   *  rather than a green tick the UI decided on by itself. */
  const testResult = ref<string | null>(null)
  const testError = ref<string | null>(null)

  const saving = ref(false)
  const switchingId = ref<string | null>(null)

  const active = computed(() => connections.value.find((c) => c.active) ?? null)

  async function fetchAll() {
    try {
      connections.value = await invoke<DbConnectionStatus[]>('list_db_connections')
    } catch (e) {
      error.value = errorMessage(e)
    }
  }

  function payload(draft: ConnectionDraft) {
    return {
      request: {
        name: draft.name.trim(),
        host: draft.host.trim(),
        port: draft.port,
        user: draft.user.trim(),
        // An empty box means "no password", not an empty one.
        password: draft.password.length > 0 ? draft.password : null,
        engine: draft.engine,
        tlsMode: draft.tlsMode,
        readOnly: draft.readOnly,
        savePassword: draft.savePassword,
        ssh: draft.useSsh
          ? {
              host: draft.sshHost.trim(),
              port: draft.sshPort,
              user: draft.sshUser.trim(),
              auth:
                draft.sshAuth === 'key'
                  ? { kind: 'key', path: draft.sshKeyPath.trim() }
                  : { kind: 'password' },
            }
          : null,
        // Separate from the database password above: the box you log into
        // and the database running on it are two different accounts.
        sshPassword: draft.useSsh && draft.sshAuth === 'password' ? draft.sshPassword : null,
      },
    }
  }

  /** Connects with the form's current values and reports the server version. */
  async function test(draft: ConnectionDraft) {
    testing.value = true
    testError.value = null
    testResult.value = null
    try {
      testResult.value = await invoke<string>('test_db_connection', payload(draft))
      return true
    } catch (e) {
      testError.value = errorMessage(e)
      return false
    } finally {
      testing.value = false
    }
  }

  async function add(draft: ConnectionDraft) {
    saving.value = true
    error.value = null
    try {
      connections.value = await invoke<DbConnectionStatus[]>('add_db_connection', payload(draft))
      return true
    } catch (e) {
      error.value = errorMessage(e)
      return false
    } finally {
      saving.value = false
    }
  }

  async function remove(id: string) {
    error.value = null
    try {
      connections.value = await invoke<DbConnectionStatus[]>('remove_db_connection', { id })
      return true
    } catch (e) {
      error.value = errorMessage(e)
      return false
    }
  }

  /** Supplies the password for a connection saved without one. */
  async function unlock(id: string, password: string, save: boolean) {
    saving.value = true
    error.value = null
    try {
      connections.value = await invoke<DbConnectionStatus[]>('set_db_connection_password', {
        id,
        password,
        save,
      })
      return true
    } catch (e) {
      error.value = errorMessage(e)
      return false
    } finally {
      saving.value = false
    }
  }

  /**
   * Points the Databases page at a remote connection.
   *
   * Nothing is stopped or started: the local server keeps running on its own
   * datadir, so unlike a profile switch this can't fail halfway.
   */
  async function use(id: string) {
    switchingId.value = id
    error.value = null
    notice.value = null
    try {
      const result = await invoke<TargetResult>('use_db_connection', { id })
      connections.value = result.connections
      notice.value = `Now reading ${result.label}.`
      return true
    } catch (e) {
      error.value = errorMessage(e)
      return false
    } finally {
      switchingId.value = null
    }
  }

  /** Points the page back at the local server. */
  async function useLocal() {
    error.value = null
    notice.value = null
    try {
      const result = await invoke<TargetResult>('use_local_db_profile')
      connections.value = result.connections
      notice.value = `Now serving ${result.label}.`
      return true
    } catch (e) {
      error.value = errorMessage(e)
      return false
    }
  }

  /** Called after a local profile switch, which clears the remote target on
   *  the Rust side — the rows here would otherwise still show one active. */
  function clearActiveLocally() {
    connections.value = connections.value.map((c) => ({ ...c, active: false }))
  }

  return {
    connections,
    active,
    error,
    notice,
    testing,
    testResult,
    testError,
    saving,
    switchingId,
    fetchAll,
    test,
    add,
    remove,
    unlock,
    use,
    useLocal,
    clearActiveLocally,
  }
})
