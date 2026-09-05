import { defineStore } from 'pinia'
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { ChangelogEntry } from '@/types/changelog'

export const useChangelogStore = defineStore('changelog', () => {
  const entries = ref<ChangelogEntry[]>([])
  const lastSeenVersion = ref<string | null>(null)
  const loading = ref(false)

  async function fetchAll() {
    loading.value = true
    try {
      entries.value = await invoke<ChangelogEntry[]>('fetch_changelog')
      lastSeenVersion.value = await invoke<string | null>('last_seen_changelog_version')
    } finally {
      loading.value = false
    }
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

  return { entries, lastSeenVersion, loading, fetchAll, markSeen }
})
