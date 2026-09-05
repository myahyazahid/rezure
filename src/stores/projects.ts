import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { LinkPreview, ProjectInfo, ProjectTemplate } from '@/types/project'
import type { ProjectDiagnosis } from '@/types/php'

function errorMessage(e: unknown): string {
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  return 'Something went wrong.'
}

export const useProjectsStore = defineStore('projects', () => {
  const projects = ref<ProjectInfo[]>([])
  const syncingHosts = ref(false)
  const hostsError = ref<string | null>(null)
  const openError = ref<string | null>(null)

  /** The project whose requirements check is open, or null. One at a time:
   *  the result is about a single project and shown in a modal. */
  const doctorFor = ref<string | null>(null)
  const diagnosis = ref<ProjectDiagnosis | null>(null)
  const doctorError = ref<string | null>(null)

  const templates = ref<ProjectTemplate[]>([])
  const wwwRoot = ref('')
  const creating = ref(false)
  const createError = ref<string | null>(null)
  const linking = ref(false)
  const linkError = ref<string | null>(null)

  const allHostsReady = computed(
    () => projects.value.length > 0 && projects.value.every((p) => p.hasHostsEntry),
  )

  /** Projects whose domain won't resolve in a browser yet — what the
   *  hosts-file prompt on the Projects page is offering to fix. */
  // A project that can't be served will never get a hosts entry either, so
  // counting it here would leave "Sync hosts file" lit with nothing to do.
  const unresolvedProjects = computed(() =>
    projects.value.filter((p) => !p.hasHostsEntry && !p.domainInvalid),
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

  /**
   * Hands a project to the browser / Explorer / a terminal. Only the
   * project id crosses the IPC boundary — Rust resolves it back to a real
   * scanned project and decides what to actually open, so nothing here
   * needs to build a URL or a shell command.
   */
  async function launch(
    command: 'open_project_site' | 'open_project_folder' | 'open_project_terminal',
    id: string,
  ) {
    openError.value = null
    try {
      await invoke(command, { id })
    } catch (e) {
      openError.value = errorMessage(e)
    }
  }

  const openSite = (id: string) => launch('open_project_site', id)
  const openFolder = (id: string) => launch('open_project_folder', id)
  const openTerminal = (id: string) => launch('open_project_terminal', id)

  /**
   * Reads a project's `ext-*` requirements back against the active PHP.
   *
   * Never runs on its own: it spawns `php -m`, so doing it for every card on
   * every visit to this page would mean a process per project. It answers a
   * question the user has just asked, which is also when the answer matters.
   */
  async function runDoctor(id: string) {
    doctorFor.value = id
    diagnosis.value = null
    doctorError.value = null
    try {
      diagnosis.value = await invoke<ProjectDiagnosis>('diagnose_project', { id })
    } catch (e) {
      doctorError.value = errorMessage(e)
    }
  }

  function closeDoctor() {
    doctorFor.value = null
    diagnosis.value = null
    doctorError.value = null
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

  /** Validates a folder and reports what linking it would produce. Throws
   *  with a readable reason when the path can't be used. */
  function previewLink(path: string) {
    return invoke<LinkPreview>('preview_project_link', { path })
  }

  /** Registers a folder outside www as a project. Records the path only —
   *  nothing inside the folder is touched. */
  async function linkProject(path: string, name?: string, domain?: string) {
    linking.value = true
    linkError.value = null
    try {
      await invoke('link_project', { path, name: name ?? null, domain: domain ?? null })
      await fetchAll()
      return true
    } catch (e) {
      linkError.value = errorMessage(e)
      return false
    } finally {
      linking.value = false
    }
  }

  /** Forgets a linked project. The folder itself is left alone. */
  async function unlinkProject(id: string) {
    linkError.value = null
    try {
      await invoke('unlink_project', { id })
      await fetchAll()
      return true
    } catch (e) {
      linkError.value = errorMessage(e)
      return false
    }
  }

  return {
    projects,
    linking,
    linkError,
    previewLink,
    linkProject,
    unlinkProject,
    syncingHosts,
    hostsError,
    openError,
    allHostsReady,
    unresolvedProjects,
    doctorFor,
    diagnosis,
    doctorError,
    runDoctor,
    closeDoctor,
    templates,
    wwwRoot,
    creating,
    createError,
    fetchAll,
    syncHosts,
    openSite,
    openFolder,
    openTerminal,
    fetchTemplateInfo,
    createProject,
  }
})
