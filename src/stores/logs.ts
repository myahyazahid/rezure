import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import type { LogEntry, LogLevel } from '@/types/log'

/**
 * Placeholder combined log tail — mirrors the hardcoded lines in
 * `ServiceLogPanel.vue` until Phase 2's real-time log streaming (Tauri
 * events from each spawned service process) lands.
 */
export const LOG_SERVICES = ['nginx', 'apache', 'mysql', 'redis', 'php-fpm'] as const

const SEED: Omit<LogEntry, 'id'>[] = [
  { time: '20:14:07', service: 'mysql', level: 'warn', message: 'slow query 1.2s — SELECT COUNT(*) FROM sessions' },
  { time: '20:14:05', service: 'mysql', level: 'info', message: 'query ok — 0.002s' },
  { time: '12:11:52', service: 'nginx', level: 'info', message: 'GET /assets/app.css 200 3ms' },
  { time: '12:11:47', service: 'nginx', level: 'info', message: 'GET /index.php 200 12ms' },
  { time: '12:11:12', service: 'mysql', level: 'warn', message: 'slow query 1.8s — SELECT * FROM orders WHERE status IS NULL' },
  {
    time: '12:10:58',
    service: 'php-fpm',
    level: 'error',
    message: 'PHP Fatal error: Uncaught TypeError in app/Http/Controllers/CartController.php:88',
  },
  { time: '12:10:31', service: 'redis', level: 'info', message: 'background saving terminated with success' },
  { time: '12:09:15', service: 'mysql', level: 'info', message: 'query ok — 0.004s' },
  { time: '12:08:02', service: 'nginx', level: 'warn', message: 'upstream timed out (110) while reading response header' },
  { time: '12:04:01', service: 'nginx', level: 'info', message: 'listening on 0.0.0.0:80' },
  { time: '12:04:01', service: 'nginx', level: 'info', message: 'worker process 4821 started' },
  { time: '12:00:05', service: 'redis', level: 'info', message: 'ready to accept connections' },
  { time: '12:00:04', service: 'mysql', level: 'info', message: 'ready for connections' },
  { time: '12:00:03', service: 'mysql', level: 'info', message: 'InnoDB initialized in 0.4s' },
  { time: '11:59:58', service: 'apache', level: 'info', message: 'apache2 -k start — resuming normal operations' },
]

const TAIL_POOL: Omit<LogEntry, 'id' | 'time'>[] = [
  { service: 'nginx', level: 'info', message: 'GET /favicon.ico 200 1ms' },
  { service: 'mysql', level: 'info', message: 'query ok — 0.003s' },
  { service: 'redis', level: 'info', message: 'keyspace notification: expired session:9f21' },
  { service: 'php-fpm', level: 'info', message: 'request completed — 38ms' },
  { service: 'nginx', level: 'warn', message: 'client closed connection while reading request' },
  { service: 'mysql', level: 'warn', message: 'slow query 1.1s — SELECT * FROM products WHERE stock < 5' },
  { service: 'php-fpm', level: 'error', message: 'PHP Warning: Undefined array key "coupon" in CartController.php:41' },
]

function timestamp() {
  return new Date().toLocaleTimeString('en-GB', { hour12: false })
}

export const useLogsStore = defineStore('logs', () => {
  const entries = ref<LogEntry[]>(SEED.map((entry, i) => ({ id: i, ...entry })))
  const paused = ref(false)
  let nextId = entries.value.length

  const errorCount = computed(() => entries.value.filter((e) => e.level === 'error').length)

  setInterval(() => {
    if (paused.value) return
    // Index is always within bounds — drawn from `TAIL_POOL`'s own length.
    const template = TAIL_POOL[Math.floor(Math.random() * TAIL_POOL.length)]!
    nextId += 1
    entries.value.unshift({ id: nextId, time: timestamp(), ...template })
  }, 2600)

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
