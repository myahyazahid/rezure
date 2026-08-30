import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { DatabaseInfo, DatabaseServerInfo, DbClientInfo } from '@/types/database'

function errorMessage(e: unknown): string {
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  return 'Something went wrong.'
}

/** MariaDB's "can't reach the server" codes. Worth singling out because it
 *  isn't a failure the user did anything wrong to cause — the service is
 *  just stopped, and the page can say so instead of showing a raw error. */
const CONNECTION_REFUSED = /\(2002\)|\(2003\)|Can't connect/i

export const useDatabasesStore = defineStore('databases', () => {
  const databases = ref<DatabaseInfo[]>([])
  const server = ref<DatabaseServerInfo | null>(null)
  const clients = ref<DbClientInfo[]>([])
  const collations = ref<string[]>([])

  const loading = ref(false)
  const error = ref<string | null>(null)
  const notice = ref<string | null>(null)
  /** Name of the database a long-running action is currently working on,
   *  so only that row shows a pending state. */
  const busy = ref<string | null>(null)

  const creating = ref(false)
  const createError = ref<string | null>(null)
  const importing = ref(false)
  const importError = ref<string | null>(null)

  const serverDown = computed(() => error.value !== null && CONNECTION_REFUSED.test(error.value))
  const totalTables = computed(() => databases.value.reduce((sum, db) => sum + db.tableCount, 0))

  /** The client named in the page subtitle — whichever real GUI was found
   *  first, rather than the bundled console fallback. */
  const preferredClient = computed(
    () => clients.value.find((client) => client.id !== 'mariadb-cli') ?? null,
  )

  async function fetchAll() {
    loading.value = true
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

  /** Dumps to `~/rezure/dumps` and reports back where the file landed. */
  async function exportDatabase(name: string) {
    busy.value = name
    error.value = null
    notice.value = null
    try {
      const path = await invoke<string>('export_database', { name })
      notice.value = `Exported ${name} to ${path}`
    } catch (e) {
      error.value = errorMessage(e)
    } finally {
      busy.value = null
    }
  }

  async function importSql(name: string, file: string) {
    importing.value = true
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
    error,
    notice,
    busy,
    creating,
    createError,
    importing,
    importError,
    serverDown,
    totalTables,
    preferredClient,
    fetchAll,
    fetchCollations,
    createDatabase,
    exportDatabase,
    importSql,
    openInClient,
    openDumpsFolder,
  }
})
