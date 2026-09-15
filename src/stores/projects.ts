import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { LinkPreview, ProjectInfo, ProjectTemplate } from '@/types/project'
import type { ProjectDiagnosis } from '@/types/php'
import { useServicesStore } from '@/stores/services'

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

  /** Project id -> public `https://*.trycloudflare.com` URL, for whichever
   *  projects currently have an active Share tunnel. */
  const shareUrls = ref<Record<string, string>>({})
  /** The project whose share is currently starting — the first click also
   *  downloads cloudflared, so this can take a few seconds. */
  const sharingFor = ref<string | null>(null)
  const shareError = ref<string | null>(null)
  /** The project whose Share details (URL, copy, stop) modal is open, or
   *  null. One at a time, same shape as `doctorFor` below. */
  const shareModalFor = ref<string | null>(null)

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
    // `list_projects` is also what registers/removes the pooled PHP service
    // for each pinned version, so the services list is stale until refetched.
    await useServicesStore().fetchAll()
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

  const phpVersionError = ref<string | null>(null)
  /** The project whose PHP version picker is open, or null — one at a time,
   *  same shape as `doctorFor`/`shareModalFor` above. */
  const phpVersionModalFor = ref<string | null>(null)
  const settingPhpVersion = ref(false)

  /**
   * Pins (or, with `version: null`, clears back to the global default) the
   * PHP version this project is served by. Distinct projects on distinct
   * pinned versions each get their own concurrently-running `php-cgi` — see
   * `services::php_pool` on the Rust side. Refetches the list so the card
   * reflects the new port/version immediately.
   */
  async function setPhpVersion(id: string, version: string | null) {
    settingPhpVersion.value = true
    phpVersionError.value = null
    try {
      await invoke('set_project_php_version', { id, version })
      await fetchAll()
      phpVersionModalFor.value = null
      return true
    } catch (e) {
      phpVersionError.value = errorMessage(e)
      return false
    } finally {
      settingPhpVersion.value = false
    }
  }

  function openPhpVersionModal(id: string) {
    phpVersionError.value = null
    phpVersionModalFor.value = id
  }

  function closePhpVersionModal() {
    phpVersionModalFor.value = null
    phpVersionError.value = null
  }

  /**
   * Starts sharing a project publicly via a Cloudflare Quick Tunnel, or
   * reuses one already running for it. The first call for a fresh install
   * also downloads cloudflared, so this can take a few seconds — `sharingFor`
   * is what a card uses to show a spinner instead of looking stuck. Opens
   * the share modal immediately, before the result is known, so the modal
   * itself carries the loading state rather than the click just looking
   * like nothing happened for several seconds.
   */
  async function shareProject(id: string) {
    sharingFor.value = id
    shareError.value = null
    shareModalFor.value = id
    try {
      const url = await invoke<string>('share_project', { id })
      shareUrls.value = { ...shareUrls.value, [id]: url }
    } catch (e) {
      shareError.value = errorMessage(e)
    } finally {
      sharingFor.value = null
    }
  }

  /** Stops a project's share tunnel, if it has one. */
  async function stopSharing(id: string) {
    await invoke('stop_sharing', { id })
    const remaining = { ...shareUrls.value }
    delete remaining[id]
    shareUrls.value = remaining
    if (shareModalFor.value === id) shareModalFor.value = null
  }

  function openShareModal(id: string) {
    shareError.value = null
    shareModalFor.value = id
  }

  function closeShareModal() {
    shareModalFor.value = null
    shareError.value = null
  }

  /**
   * Repopulates `shareUrls` from whatever's actually still running —
   * without this, reloading the Projects page while a share is active would
   * show it as "not shared" even though `cloudflared.exe` is still up.
   */
  async function restoreShareStatus() {
    const entries = await Promise.all(
      projects.value.map(
        async (p) => [p.id, await invoke<string | null>('sharing_status', { id: p.id })] as const,
      ),
    )
    const active: Record<string, string> = {}
    for (const [id, url] of entries) {
      if (url) active[id] = url
    }
    shareUrls.value = active
  }

  return {
    projects,
    linking,
    linkError,
    previewLink,
    linkProject,
    unlinkProject,
    phpVersionError,
    phpVersionModalFor,
    settingPhpVersion,
    setPhpVersion,
    openPhpVersionModal,
    closePhpVersionModal,
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
    shareUrls,
    sharingFor,
    shareError,
    shareModalFor,
    shareProject,
    stopSharing,
    openShareModal,
    closeShareModal,
    restoreShareStatus,
  }
})
