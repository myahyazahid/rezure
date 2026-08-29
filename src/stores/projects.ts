import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { ProjectInfo } from '@/types/project'

export const useProjectsStore = defineStore('projects', () => {
  const projects = ref<ProjectInfo[]>([])
  const syncingHosts = ref(false)
  const hostsError = ref<string | null>(null)

  const allHostsReady = computed(
    () => projects.value.length > 0 && projects.value.every((p) => p.hasHostsEntry),
  )

  async function fetchAll() {
    projects.value = await invoke<ProjectInfo[]>('list_projects')
  }

  /**
   * Writes every project's domain into the OS hosts file. Triggers a real
   * Windows admin (UAC) prompt — never called automatically, only from an
   * explicit user action, since a system-file write shouldn't be a
   * surprise side effect of opening this page.
   */
  async function syncHosts() {
    syncingHosts.value = true
    hostsError.value = null
    try {
      await invoke<boolean>('sync_hosts')
      await fetchAll()
    } catch (e) {
      hostsError.value = typeof e === 'string' ? e : e instanceof Error ? e.message : String(e)
    } finally {
      syncingHosts.value = false
    }
  }

  return { projects, syncingHosts, hostsError, allHostsReady, fetchAll, syncHosts }
})
