import { defineStore } from 'pinia'
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { OdooInstall } from '@/types/database'

/** What can be in flight for Odoo — one thing at a time. */
type Busy = 'connect' | 'restore' | 'start' | 'stop' | 'restart' | 'allow' | 'repair'
/** Mirrors `windows_service_control::Action`. */
export type ServiceAction = 'start' | 'stop' | 'restart'
import { useServicesStore } from '@/stores/services'

function errorMessage(e: unknown): string {
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  return 'Something went wrong.'
}

/**
 * Odoo installed on this PC, and pointing it at Rezure's PostgreSQL. All of
 * it — detection, rewriting odoo.conf, the services — is `services::odoo`;
 * this only asks and shows.
 */
export const useOdooStore = defineStore('odoo', () => {
  const installs = ref<OdooInstall[]>([])
  const detected = ref(false)
  const error = ref<string | null>(null)
  const busy = ref<Busy | null>(null)
  /** A one-line confirmation of the last action whose result isn't
   *  otherwise visible (a restart). */
  const notice = ref<string | null>(null)

  async function detect() {
    try {
      installs.value = await invoke<OdooInstall[]>('detect_odoo')
    } catch (e) {
      error.value = errorMessage(e)
    } finally {
      detected.value = true
    }
  }

  /** Both change which PostgreSQL runs, so the Services page is re-read. */
  async function run(kind: Busy, action: () => Promise<OdooInstall[]>) {
    busy.value = kind
    error.value = null
    notice.value = null
    try {
      installs.value = await action()
      return true
    } catch (e) {
      error.value = errorMessage(e)
      await detect()
      return false
    } finally {
      busy.value = null
      await useServicesStore()
        .fetchAll()
        .catch(() => {})
    }
  }

  const connect = (dir: string, stopBundledPostgres: boolean) =>
    run('connect', () =>
      invoke<OdooInstall[]>('connect_odoo', { dir, options: { stopBundledPostgres } }),
    )

  const restore = (dir: string) =>
    run('restore', () => invoke<OdooInstall[]>('restore_odoo', { dir }))

  const DONE: Record<ServiceAction, string> = {
    start: 'Odoo started.',
    stop: 'Odoo stopped.',
    restart: 'Odoo restarted - it has read odoo.conf again.',
  }

  /** Starts, stops or restarts Odoo's service — without a prompt once
   *  `allowControl` has run, through UAC before that. Starting brings
   *  Rezure's PostgreSQL up first. */
  async function control(dir: string, action: ServiceAction) {
    const ok = await run(action, () => invoke<OdooInstall[]>('control_odoo', { dir, action }))
    if (ok) notice.value = DONE[action]
    return ok
  }

  /** One UAC prompt now, none for Start/Stop/Restart afterwards. */
  async function allowControl(dir: string) {
    const ok = await run('allow', () => invoke<OdooInstall[]>('allow_odoo_control', { dir }))
    if (ok) notice.value = 'Done - Start, Stop and Restart no longer ask for admin.'
    return ok
  }

  /** Recreates Odoo's login on Rezure's PostgreSQL from odoo.conf — no
   *  admin prompt, nothing under Program Files changes. */
  async function repairLogin(dir: string) {
    const ok = await run('repair', () => invoke<OdooInstall[]>('repair_odoo_login', { dir }))
    if (ok) notice.value = "Odoo's login is back - reload Odoo in the browser."
    return ok
  }

  async function open(dir: string) {
    error.value = null
    try {
      await invoke('open_odoo', { dir })
    } catch (e) {
      error.value = errorMessage(e)
    }
  }

  /** odoo.conf in Notepad run as administrator — for the master password,
   *  which Rezure leaves to the user. */
  async function openConf(dir: string) {
    error.value = null
    try {
      await invoke('open_odoo_conf', { dir })
    } catch (e) {
      error.value = errorMessage(e)
    }
  }

  return {
    installs,
    detected,
    error,
    notice,
    busy,
    detect,
    connect,
    restore,
    control,
    allowControl,
    repairLogin,
    open,
    openConf,
  }
})
