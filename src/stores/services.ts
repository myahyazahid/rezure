import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { ServiceInfo } from '@/types/service'

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
    restart,
    startAll,
    stopAll,
    isPending,
  }
})
