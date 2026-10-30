import { defineStore } from 'pinia'
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { ChangelogEntry } from '@/types/changelog'

export const useChangelogStore = defineStore('changelog', () => {
  const entries = ref<ChangelogEntry[]>([])
  const lastSeenVersion = ref<string | null>(null)
  const loading = ref(false)

  /** Versions whose body the Changelog page shows. Kept here rather than in
   *  the view so a release the user collapsed stays collapsed when they come
   *  back to the page, for as long as the app runs. */
  const expandedVersions = ref(new Set<string>())
  /** Every version the page has already laid out once; the user's own
   *  open/closed choice for these is never overridden. */
  const knownVersions = new Set<string>()

  async function fetchAll() {
    loading.value = true
    try {
      entries.value = await invoke<ChangelogEntry[]>('fetch_changelog')
      lastSeenVersion.value = await invoke<string | null>('last_seen_changelog_version')
      openNewRelease()
    } finally {
      loading.value = false
    }
  }

  /** The newest release starts open the first time it is listed — that is
   *  what someone opening this page came to read. Anything listed before
   *  keeps whatever state the user left it in. */
  function openNewRelease() {
    const newest = entries.value[0]?.version
    if (newest && !knownVersions.has(newest)) {
      expandedVersions.value = new Set(expandedVersions.value).add(newest)
    }
    for (const entry of entries.value) knownVersions.add(entry.version)
  }

  function toggleExpanded(version: string) {
    const next = new Set(expandedVersions.value)
    if (!next.delete(version)) next.add(version)
    expandedVersions.value = next
  }

  async function markSeen() {
    const newest = entries.value[0]?.version
    if (!newest || newest === lastSeenVersion.value) return
    try {
      await invoke('mark_changelog_seen', { version: newest })
      lastSeenVersion.value = newest
    } catch {
      // Informational only — the badge just stays lit until next visit.
    }
  }

  return {
    entries,
    lastSeenVersion,
    loading,
    expandedVersions,
    fetchAll,
    markSeen,
    toggleExpanded,
  }
})
