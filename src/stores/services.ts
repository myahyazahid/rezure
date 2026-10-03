import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { ManagedService, PortHolder, ServiceInfo } from '@/types/service'

// Keep in sync with `CHANGED_EVENT` in src-tauri/src/services/supervisor.rs
const CHANGED_EVENT = 'service://changed'

export const useServicesStore = defineStore('services', () => {
  /** The Services page's rows: every service not removed with Manage
   *  services. */
  const services = ref<ServiceInfo[]>([])
  /** The Manage services list — every service, shown or not. */
  const managed = ref<ManagedService[]>([])
  const loading = ref(false)
  const pendingIds = ref<Set<string>>(new Set())

  const runningCount = computed(() => services.value.filter((s) => s.status === 'running').length)

  /** True while any service is mid start/stop/restart. Drives the busy
   *  overlay: a bulk action leaves the window looking frozen otherwise,
   *  because spawning nginx, php-cgi and mysqld takes real seconds. */
  const busy = computed(() => pendingIds.value.size > 0)

  async function fetchAll() {
    // Spinner only on a first load — a refetch leaves the current rows up so
    // the dashboard doesn't blank out on the way back to it.
    loading.value = services.value.length === 0
    try {
      services.value = await invoke<ServiceInfo[]>('list_services')
    } finally {
      loading.value = false
    }
  }

  // The backend noticed a crash, restarted a service or gave up on one
  // without the UI asking — refetch so a dead PHP doesn't still read as
  // Running.
  listen(CHANGED_EVENT, () => {
    fetchAll()
  })

  async function withPending(id: string, action: () => Promise<ServiceInfo>) {
    pendingIds.value.add(id)
    try {
      const updated = await action()
      const index = services.value.findIndex((s) => s.id === id)
      if (index !== -1) services.value[index] = updated
    } finally {
      pendingIds.value.delete(id)
    }
  }

  function start(id: string) {
    return withPending(id, () => invoke<ServiceInfo>('start_service', { id }))
  }

  function stop(id: string) {
    return withPending(id, () => invoke<ServiceInfo>('stop_service', { id }))
  }

  /** Kills the process without waiting for a clean shutdown. The caller is
   *  expected to have confirmed with the user first — for a database this
   *  leaves the data directory needing crash recovery. */
  function forceStop(id: string) {
    return withPending(id, () => invoke<ServiceInfo>('force_stop_service', { id }))
  }

  /** Who is holding a port, so a "port in use" failure can name the culprit
   *  instead of leaving the user to hunt for it. Null when it's free. */
  function portHolder(port: number) {
    return invoke<PortHolder | null>('port_holder', { port })
  }

  /** Kills whatever holds `port`. Returns whoever still holds it after —
   *  normally null. Starting the service stays a separate step. */
  function freePort(port: number) {
    return invoke<PortHolder | null>('free_port', { port })
  }

  function restart(id: string) {
    return withPending(id, () => invoke<ServiceInfo>('restart_service', { id }))
  }

  /** Starts every stopped service that's installed. An optional service
   *  nobody has downloaded yet (Mailpit) is skipped rather than failing the
   *  whole action. */
  function startAll() {
    return Promise.all(
      services.value.filter((s) => s.status !== 'running' && s.installed).map((s) => start(s.id)),
    )
  }

  /** Opens a running service's web UI. The address is looked up on the Rust
   *  side from the service itself. */
  function openUi(id: string) {
    return invoke<void>('open_service_ui', { id })
  }

  function stopAll() {
    return Promise.all(services.value.filter((s) => s.status === 'running').map((s) => stop(s.id)))
  }

  /** Restarts every running service. Stopped ones stay stopped — that's
   *  what Start all is for. */
  function restartAll() {
    return Promise.all(
      services.value.filter((s) => s.status === 'running').map((s) => restart(s.id)),
    )
  }

  async function fetchManaged() {
    managed.value = await invoke<ManagedService[]>('list_managed_services')
  }

  /** Adds a service to the Services page or removes it. Removing a running
   *  one stops it first, on the Rust side. */
  async function setShown(id: string, shown: boolean) {
    managed.value = await invoke<ManagedService[]>('set_service_shown', { id, shown })
    await fetchAll()
  }

  function isPending(id: string) {
    return pendingIds.value.has(id)
  }

  return {
    services,
    managed,
    loading,
    runningCount,
    busy,
    fetchAll,
    start,
    stop,
    forceStop,
    portHolder,
    freePort,
    restart,
    startAll,
    openUi,
    stopAll,
    restartAll,
    fetchManaged,
    setShown,
    isPending,
  }
})
