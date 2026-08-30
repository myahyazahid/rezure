import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { PortHolder, ServiceInfo } from '@/types/service'

export const useServicesStore = defineStore('services', () => {
  const services = ref<ServiceInfo[]>([])
  const loading = ref(false)
  const pendingIds = ref<Set<string>>(new Set())

  const runningCount = computed(() => services.value.filter((s) => s.status === 'running').length)

  async function fetchAll() {
    loading.value = true
    try {
      services.value = await invoke<ServiceInfo[]>('list_services')
    } finally {
      loading.value = false
    }
  }

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

  function startAll() {
    return Promise.all(services.value.filter((s) => s.status !== 'running').map((s) => start(s.id)))
  }

  function stopAll() {
    return Promise.all(services.value.filter((s) => s.status === 'running').map((s) => stop(s.id)))
  }

  function isPending(id: string) {
    return pendingIds.value.has(id)
  }

  return {
    services,
    loading,
    runningCount,
    fetchAll,
    start,
    stop,
    forceStop,
    portHolder,
    freePort,
    restart,
    startAll,
    stopAll,
    isPending,
  }
})
