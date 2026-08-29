import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { ProjectInfo, ProjectTemplate } from '@/types/project'

function errorMessage(e: unknown): string {
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  return 'Something went wrong.'
}

export const useProjectsStore = defineStore('projects', () => {
  const projects = ref<ProjectInfo[]>([])
  const syncingHosts = ref(false)
  const hostsError = ref<string | null>(null)

  const templates = ref<ProjectTemplate[]>([])
  const wwwRoot = ref('')
  const creating = ref(false)
  const createError = ref<string | null>(null)

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
      hostsError.value = errorMessage(e)
    } finally {
      syncingHosts.value = false
    }
  }

  async function fetchTemplateInfo() {
    const [fetchedTemplates, fetchedWwwRoot] = await Promise.all([
      invoke<ProjectTemplate[]>('list_project_templates'),
      invoke<string>('www_root'),
    ])
    templates.value = fetchedTemplates
    wwwRoot.value = fetchedWwwRoot
  }

  /** Scaffolds a new project from a template. Laravel can take a while —
   *  it resolves and downloads Composer dependencies over the network. */
  async function createProject(name: string, templateId: string) {
    creating.value = true
    createError.value = null
    try {
      await invoke('create_project', { name, template: templateId })
      await fetchAll()
      return true
    } catch (e) {
      createError.value = errorMessage(e)
      return false
    } finally {
      creating.value = false
    }
  }

  return {
    projects,
    syncingHosts,
    hostsError,
    allHostsReady,
    templates,
    wwwRoot,
    creating,
    createError,
    fetchAll,
    syncHosts,
    fetchTemplateInfo,
    createProject,
  }
})
