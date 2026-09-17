import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type {
  DatabaseInfo,
  DatabaseServerInfo,
  DbClientInfo,
  ExportProgress,
} from '@/types/database'

// Keep in sync with `EXPORT_PROGRESS_EVENT` in src-tauri/src/services/database.rs
const EXPORT_PROGRESS_EVENT = 'database://export-progress'

function errorMessage(e: unknown): string {
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  return 'Something went wrong.'
}

/** The "can't reach the server" shapes. Worth singling out because it isn't
 *  a failure the user did anything wrong to cause — the service is just
 *  stopped, or a remote host is unreachable, and the page can say so instead
 *  of showing a raw error.
 *
 *  Both spellings are matched: the raw client codes, and the rewritten
 *  message the Rust side produces for a failed connect. */
const CONNECTION_REFUSED = /\(2002\)|\(2003\)|Can't connect|can't reach/i

export const useDatabasesStore = defineStore('databases', () => {
  const databases = ref<DatabaseInfo[]>([])
  const server = ref<DatabaseServerInfo | null>(null)
  const clients = ref<DbClientInfo[]>([])
  const collations = ref<string[]>([])

  const loading = ref(false)
  /** True for the whole of any `fetchAll`, first load or not.
   *
   *  Separate from `loading`, which is deliberately false on a refetch so the
   *  existing rows stay on screen. The manual Refresh button still needs to
   *  show it is doing something, and `loading` can't tell it. */
  const refreshing = ref(false)
  const error = ref<string | null>(null)
  const notice = ref<string | null>(null)
  /** Name of the database a long-running action is currently working on,
   *  so only that row shows a pending state. */
  const busy = ref<string | null>(null)
  /** Latest progress tick for the export named by `busy`, or `null` before
   *  the first tick lands (or once the export has ended). */
  const exportProgress = ref<ExportProgress | null>(null)

  const creating = ref(false)
  const createError = ref<string | null>(null)
  const importing = ref(false)
  const importError = ref<string | null>(null)
  /** Which database an import is loading into, for the busy label. `importing`
   *  stays the boolean the modal's own controls read. */
  const importingInto = ref<string | null>(null)

  const serverDown = computed(() => error.value !== null && CONNECTION_REFUSED.test(error.value))
  const totalTables = computed(() => databases.value.reduce((sum, db) => sum + db.tableCount, 0))

  /** 0-100, or `null` before the first progress tick or when the schema's
   *  size couldn't be read up front.
   *
   *  Capped below 100 while still running: `estimatedTotalBytes` is the
   *  schema's raw storage size, and a `.sql` dump — text, `INSERT`
   *  statements, hex-escaped BLOBs — almost never lands on that exact byte
   *  count. A bar that hits 100% and then keeps churning for another minute
   *  reads as broken, so the last few percent are reserved for the export
   *  actually finishing. */
  const exportPercent = computed(() => {
    const progress = exportProgress.value
    if (!progress || !progress.estimatedTotalBytes) return null
    const raw = (progress.bytesWritten / progress.estimatedTotalBytes) * 100
    return Math.min(95, Math.round(raw))
  })

  listen<ExportProgress>(EXPORT_PROGRESS_EVENT, (event) => {
    // Guards against a straggling tick from a just-finished or just-
    // cancelled export landing after `busy` has already moved on.
    if (event.payload.name === busy.value) exportProgress.value = event.payload
  })

  /** The client named in the page subtitle — whichever real GUI was found
   *  first, rather than the bundled console fallback. */
  const preferredClient = computed(
    () => clients.value.find((client) => client.id !== 'mariadb-cli') ?? null,
  )

  async function fetchAll() {
    // Stale-while-revalidate: only a first load, with nothing on screen yet,
    // shows the spinner. A refetch keeps the rows visible and swaps them when
    // the answer arrives — otherwise every visit to this page flashes
    // "Reading schemas…" over a list that was already correct.
    const firstLoad = databases.value.length === 0
    loading.value = firstLoad
    refreshing.value = true
    error.value = null
    try {
      const [list, info, found] = await Promise.all([
        invoke<DatabaseInfo[]>('list_databases'),
        invoke<DatabaseServerInfo>('database_server_info'),
        invoke<DbClientInfo[]>('list_db_clients'),
      ])
      databases.value = list
      server.value = info
      clients.value = found
    } catch (e) {
      error.value = errorMessage(e)
      databases.value = []
    } finally {
      loading.value = false
      refreshing.value = false
    }
  }

  async function fetchCollations() {
    try {
      collations.value = await invoke<string[]>('list_collations')
    } catch {
      // Not worth surfacing: the dialog falls back to a sensible default
      // collation when the list can't be read.
      collations.value = []
    }
  }

  async function createDatabase(name: string, collation: string) {
    creating.value = true
    createError.value = null
    try {
      await invoke('create_database', { name, collation })
      await fetchAll()
      return true
    } catch (e) {
      createError.value = errorMessage(e)
      return false
    } finally {
      creating.value = false
    }
  }

  /** Dumps to `C:
ezure\dumps` and reports back where the file landed. */
  async function exportDatabase(name: string) {
    busy.value = name
    error.value = null
    notice.value = null
    exportProgress.value = null
    try {
      const path = await invoke<string>('export_database', { name })
      notice.value = `Exported ${name} to ${path}`
    } catch (e) {
      const message = errorMessage(e)
      // The one failure the user asked for — reads as a status, not a
      // problem to fix, so it doesn't belong in the error banner.
      if (message.includes('was cancelled')) {
        notice.value = message
      } else {
        error.value = message
      }
    } finally {
      busy.value = null
      exportProgress.value = null
    }
  }

  /** Stops the export named by `busy`, if there is one. A no-op once it has
   *  already finished on its own — `exportDatabase`'s own catch/finally
   *  handles the cleanup either way. */
  async function cancelExport() {
    if (!busy.value) return
    await invoke('cancel_export', { name: busy.value })
  }

  async function importSql(name: string, file: string) {
    importing.value = true
    importingInto.value = name
    importError.value = null
    try {
      await invoke('import_sql', { name, file })
      await fetchAll()
      notice.value = `Imported ${file} into ${name}`
      return true
    } catch (e) {
      importError.value = errorMessage(e)
      return false
    } finally {
      importing.value = false
      importingInto.value = null
    }
  }

  async function openInClient(client: string, database: string) {
    error.value = null
    try {
      await invoke('open_in_db_client', { client, database })
    } catch (e) {
      error.value = errorMessage(e)
    }
  }

  async function openDumpsFolder() {
    error.value = null
    try {
      await invoke('open_dumps_folder')
    } catch (e) {
      error.value = errorMessage(e)
    }
  }

  return {
    databases,
    server,
    clients,
    collations,
    loading,
    refreshing,
    error,
    notice,
    busy,
    exportProgress,
    exportPercent,
    creating,
    createError,
    importing,
    importError,
    importingInto,
    serverDown,
    totalTables,
    preferredClient,
    fetchAll,
    fetchCollations,
    createDatabase,
    exportDatabase,
    cancelExport,
    importSql,
    openInClient,
    openDumpsFolder,
  }
})
