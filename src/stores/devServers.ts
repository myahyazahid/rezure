import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { DevServerChange, DevServerRow } from '@/types/devServer'
import type { ServiceStatus } from '@/types/service'

function errorMessage(e: unknown): string {
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  return 'Something went wrong.'
}

/** How often the Projects page re-reads which dev servers are up, so one
 *  that exits on its own doesn't keep reading as running. */
const STATUS_POLL_MS = 4000

/**
 * The dev servers behind `proxy` projects (Vite, Astro, Next, a Node app).
 * Everything here is decided in Rust (`services::dev_servers`): the port, the
 * command, whether it can start — this store only asks and shows.
 */
export const useDevServersStore = defineStore('devServers', () => {
  const rows = ref<DevServerRow[]>([])
  /** Project id -> status, polled cheaply for the Projects page's dots. */
  const statuses = ref<Record<string, ServiceStatus>>({})
  const loading = ref(false)
  const error = ref<string | null>(null)
  /** Project id -> what's in flight for it ("Starting…", "Installing…"). */
  const pending = ref<Record<string, string>>({})
  const modalOpen = ref(false)
  /** The project whose log is expanded in the dialog. */
  const logFor = ref<string | null>(null)

  const runningCount = computed(
    () => Object.values(statuses.value).filter((status) => status === 'running').length,
  )

  function statusOf(projectId: string): ServiceStatus {
    if (pending.value[projectId] === 'Starting…') return 'starting'
    return statuses.value[projectId] ?? 'stopped'
  }

  function applyRows(next: DevServerRow[]) {
    rows.value = next
    const map: Record<string, ServiceStatus> = { ...statuses.value }
    for (const row of next) map[row.projectId] = row.status
    statuses.value = map
  }

  async function fetchRows() {
    loading.value = rows.value.length === 0
    try {
      applyRows(await invoke<DevServerRow[]>('list_dev_servers'))
    } catch (e) {
      error.value = errorMessage(e)
    } finally {
      loading.value = false
    }
  }

  async function fetchStatuses() {
    try {
      statuses.value = await invoke<Record<string, ServiceStatus>>('dev_server_statuses')
    } catch {
      // A missed poll just leaves the dots as they were until the next one.
    }
  }

  let timer: ReturnType<typeof setInterval> | null = null

  function startPolling() {
    fetchStatuses()
    if (timer === null) timer = setInterval(fetchStatuses, STATUS_POLL_MS)
  }

  function stopPolling() {
    if (timer !== null) clearInterval(timer)
    timer = null
  }

  /** Runs one action for one project, with its label shown on the row. */
  async function run(projectId: string, label: string, action: () => Promise<DevServerRow[]>) {
    if (pending.value[projectId]) return false
    pending.value = { ...pending.value, [projectId]: label }
    error.value = null
    try {
      applyRows(await action())
      return true
    } catch (e) {
      error.value = errorMessage(e)
      await fetchRows()
      return false
    } finally {
      const rest = { ...pending.value }
      delete rest[projectId]
      pending.value = rest
    }
  }

  function save(projectId: string, change: DevServerChange) {
    return run(projectId, 'Saving…', () =>
      invoke<DevServerRow[]>('set_dev_server_config', { id: projectId, change }),
    )
  }

  /** Starts it and waits until it answers on its port — a first compile can
   *  take a little while. */
  function start(projectId: string) {
    logFor.value = projectId
    return run(projectId, 'Starting…', () =>
      invoke<DevServerRow[]>('start_dev_server', { id: projectId }),
    )
  }

  function stop(projectId: string) {
    return run(projectId, 'Stopping…', () =>
      invoke<DevServerRow[]>('stop_dev_server', { id: projectId }),
    )
  }

  /** `npm install` (or pnpm/yarn/bun), its output in the row's log. */
  function install(projectId: string) {
    logFor.value = projectId
    return run(projectId, 'Installing…', () =>
      invoke<DevServerRow[]>('install_dev_dependencies', { id: projectId }),
    )
  }

  /** Stops the copy of this dev server that runs outside Rezure, then starts
   *  Rezure's own in its place. */
  async function replaceOutside(projectId: string) {
    const stopped = await run(projectId, 'Stopping the other copy…', () =>
      invoke<DevServerRow[]>('stop_outside_dev_server', { id: projectId }),
    )
    if (stopped) return start(projectId)
    return false
  }

  const stoppingAll = ref(false)

  async function stopAll() {
    stoppingAll.value = true
    error.value = null
    try {
      applyRows(await invoke<DevServerRow[]>('stop_all_dev_servers'))
    } catch (e) {
      error.value = errorMessage(e)
    } finally {
      stoppingAll.value = false
    }
  }

  /** Opens the dialog — with `projectId`'s log expanded when given, which is
   *  how a failed Open points at what went wrong. */
  function openModal(projectId: string | null = null) {
    error.value = null
    logFor.value = projectId
    modalOpen.value = true
    fetchRows()
  }

  function closeModal() {
    modalOpen.value = false
  }

  /** Marks a project as starting from outside the dialog (Open). */
  function markStarting(projectId: string, starting: boolean) {
    const rest = { ...pending.value }
    if (starting) rest[projectId] = 'Starting…'
    else delete rest[projectId]
    pending.value = rest
  }

  return {
    rows,
    statuses,
    loading,
    error,
    pending,
    modalOpen,
    logFor,
    runningCount,
    stoppingAll,
    statusOf,
    fetchRows,
    fetchStatuses,
    startPolling,
    stopPolling,
    save,
    start,
    stop,
    install,
    replaceOutside,
    stopAll,
    openModal,
    closeModal,
    markStarting,
  }
})
