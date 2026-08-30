import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { listen } from '@tauri-apps/api/event'
import type { LogEntry, LogLevel, ServiceLogEvent } from '@/types/log'

export const LOG_SERVICES = ['nginx', 'php', 'mariadb'] as const

// Keep in sync with `LOG_EVENT` in src-tauri/src/services/process.rs
const LOG_EVENT = 'service://log'

/** Rolling buffer cap — a long-running dev database can log indefinitely. */
const MAX_ENTRIES = 1000

function timestamp() {
  return new Date().toLocaleTimeString('en-GB', { hour12: false })
}

/**
 * The Rust side doesn't tag lines as info/warn/error — nginx, php, and
 * mysqld all just write plain text to stdout/stderr — so this classifies
 * by content instead. Matches the `[Warning]`/`[ERROR]` tags MariaDB
 * prefixes its own lines with, and nginx's `[warn]`/`[error]` log format.
 */
function classify(line: string): LogLevel {
  const lower = line.toLowerCase()
  if (lower.includes('error') || lower.includes('fatal')) return 'error'
  if (lower.includes('warn')) return 'warn'
  return 'info'
}

export const useLogsStore = defineStore('logs', () => {
  const entries = ref<LogEntry[]>([])
  const paused = ref(false)
  let nextId = 0

  listen<ServiceLogEvent>(LOG_EVENT, (event) => {
    if (paused.value) return
    nextId += 1
    entries.value.unshift({
      id: nextId,
      time: timestamp(),
      service: event.payload.serviceId,
      level: classify(event.payload.line),
      message: event.payload.line,
    })
    if (entries.value.length > MAX_ENTRIES) {
      entries.value.length = MAX_ENTRIES
    }
  })

  const errorCount = computed(() => entries.value.filter((e) => e.level === 'error').length)

  function togglePause() {
    paused.value = !paused.value
  }

  function clear() {
    entries.value = []
  }

  function filtered(service: string | null, level: LogLevel | null, search: string) {
    const query = search.trim().toLowerCase()
    return entries.value.filter((entry) => {
      if (service && entry.service !== service) return false
      if (level && entry.level !== level) return false
      if (query && !entry.message.toLowerCase().includes(query)) return false
      return true
    })
  }

  return { entries, paused, errorCount, togglePause, clear, filtered }
})
