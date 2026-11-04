import { defineStore } from 'pinia'
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { ConnectionSnippet, NewPostgresRole, PostgresRole } from '@/types/database'

function errorMessage(e: unknown): string {
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  return 'Something went wrong.'
}

/**
 * Users on Rezure's own PostgreSQL — the logins apps connect as. Everything
 * is decided in Rust (`services::postgres_roles`): which names are allowed,
 * what a new user may do. Passwords are passed straight through and never
 * kept here beyond the dialog that typed them.
 */
export const usePostgresUsersStore = defineStore('postgresUsers', () => {
  const roles = ref<PostgresRole[]>([])
  const loading = ref(false)
  const error = ref<string | null>(null)
  /** The user being changed, or `'new'` while one is being created. */
  const busy = ref<string | null>(null)

  async function fetchAll() {
    loading.value = roles.value.length === 0
    error.value = null
    try {
      roles.value = await invoke<PostgresRole[]>('list_postgres_roles')
    } catch (e) {
      error.value = errorMessage(e)
    } finally {
      loading.value = false
    }
  }

  async function create(role: NewPostgresRole) {
    busy.value = 'new'
    error.value = null
    try {
      roles.value = await invoke<PostgresRole[]>('create_postgres_role', { role })
      return true
    } catch (e) {
      error.value = errorMessage(e)
      return false
    } finally {
      busy.value = null
    }
  }

  async function setPassword(name: string, password: string | null) {
    busy.value = name
    error.value = null
    try {
      await invoke('set_postgres_role_password', { name, password })
      return true
    } catch (e) {
      error.value = errorMessage(e)
      return false
    } finally {
      busy.value = null
    }
  }

  async function drop(name: string) {
    busy.value = name
    error.value = null
    try {
      roles.value = await invoke<PostgresRole[]>('drop_postgres_role', { name })
      return true
    } catch (e) {
      error.value = errorMessage(e)
      return false
    } finally {
      busy.value = null
    }
  }

  function snippets(user: string, password: string | null, database: string | null) {
    return invoke<ConnectionSnippet[]>('postgres_connection_snippets', {
      user,
      password,
      database,
    })
  }

  return { roles, loading, error, busy, fetchAll, create, setPassword, drop, snippets }
})
