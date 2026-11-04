<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue'
import { usePostgresUsersStore } from '@/stores/postgresUsers'
import ConnectionSnippets from '@/components/databases/ConnectionSnippets.vue'
import NewPostgresUserModal from '@/components/databases/NewPostgresUserModal.vue'
import LeafLoader from '@/components/common/LeafLoader.vue'

/**
 * Users on Rezure's own PostgreSQL: who exists, a new one (in its own modal,
 * `NewPostgresUserModal`), and the settings that point an app at one. Rules
 * — which names, what a user may do — are Rust's (`postgres_roles`).
 */
const emit = defineEmits<{ close: [] }>()

const store = usePostgresUsersStore()

/** The "new user" modal is open over this one. */
const adding = ref(false)

/** The user whose settings are shown, with the password just set for it. */
const connecting = ref<{ user: string; password: string | null; database: string | null } | null>(
  null,
)
/** The user waiting on a second click to be deleted. */
const confirmingDelete = ref<string | null>(null)

function openNew() {
  store.error = null
  adding.value = true
}

function closeNew() {
  if (store.busy === 'new') return
  adding.value = false
  // A refused create was the new modal's to show, not the list's.
  store.error = null
}

function onCreated(created: { user: string; password: string | null; database: string | null }) {
  adding.value = false
  // The moment the password is still known — show what to paste where.
  connecting.value = created
}

function showConnect(user: string, owned: string[]) {
  connecting.value =
    connecting.value?.user === user ? null : { user, password: null, database: owned[0] ?? null }
}

async function remove(user: string) {
  if (confirmingDelete.value !== user) {
    confirmingDelete.value = user
    return
  }
  confirmingDelete.value = null
  if (await store.drop(user)) {
    if (connecting.value?.user === user) connecting.value = null
  }
}

function close() {
  if (store.busy) return
  emit('close')
}

/** One listener for both modals, so Escape closes only the top one. */
function onKeydown(e: KeyboardEvent) {
  if (e.key !== 'Escape') return
  if (adding.value) closeNew()
  else close()
}

onMounted(() => {
  window.addEventListener('keydown', onKeydown)
  store.fetchAll()
})
onUnmounted(() => window.removeEventListener('keydown', onKeydown))
</script>

<template>
  <div
    class="glass-scrim fixed inset-0 z-50 flex items-center justify-center p-4"
    @click.self="close"
  >
    <div class="glass-strong flex max-h-[88vh] w-full max-w-2xl flex-col rounded-3xl p-6">
      <div class="flex items-start justify-between gap-4">
        <div>
          <h2 class="text-xl font-bold tracking-tight">PostgreSQL users</h2>
          <p class="mt-1 text-sm text-neutral-500">
            The logins apps connect to Rezure's PostgreSQL as. Give each app its own - some, like
            Odoo, refuse to run as <span class="font-mono">postgres</span>.
          </p>
        </div>
        <button
          type="button"
          class="glass-accent flex shrink-0 items-center gap-2 rounded-full px-4 py-2 text-sm font-semibold transition"
          @click="openNew"
        >
          <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2.5"
            class="h-4 w-4"
          >
            <path stroke-linecap="round" d="M12 5v14M5 12h14" />
          </svg>
          New user
        </button>
      </div>

      <div class="-mx-1 mt-4 flex-1 overflow-y-auto px-1">
        <div v-if="store.loading" class="my-8 flex justify-center">
          <LeafLoader :size="36" label="Loading users…" />
        </div>
        <p v-else-if="store.error && !adding" class="text-sm text-red-600 dark:text-red-400">
          {{ store.error }}
        </p>

        <ul v-if="!store.loading" class="flex flex-col gap-2">
          <li v-for="role in store.roles" :key="role.name" class="glass-inset rounded-2xl">
            <div class="flex items-center gap-3 px-4 py-3">
              <div class="min-w-0 flex-1">
                <div class="flex flex-wrap items-center gap-1.5">
                  <span
                    class="font-mono text-sm font-semibold text-neutral-900 dark:text-neutral-100"
                  >
                    {{ role.name }}
                  </span>
                  <span
                    v-if="role.superuser"
                    class="rounded-full bg-amber-500/15 px-2 py-0.5 text-[10px] font-semibold text-amber-700 dark:text-amber-300"
                    >superuser</span
                  >
                  <span
                    v-else-if="role.canCreateDb"
                    class="rounded-full bg-accent-500/10 px-2 py-0.5 text-[10px] font-semibold text-accent-700 dark:text-accent-300"
                    >can create databases</span
                  >
                  <span
                    v-if="role.builtin"
                    class="rounded-full bg-neutral-500/10 px-2 py-0.5 text-[10px] font-semibold text-neutral-500"
                    >comes with Rezure</span
                  >
                </div>
                <p class="mt-0.5 truncate text-xs text-neutral-500">
                  {{
                    role.ownedDatabases.length
                      ? `Owns ${role.ownedDatabases.join(', ')}`
                      : 'Owns no databases'
                  }}
                </p>
              </div>
              <button
                type="button"
                class="glass-btn shrink-0 rounded-full px-3 py-1.5 text-xs font-semibold text-neutral-700 transition dark:text-neutral-200"
                :class="connecting?.user === role.name ? 'glass-raised' : ''"
                @click="showConnect(role.name, role.ownedDatabases)"
              >
                Connect an app
              </button>
              <button
                v-if="!role.builtin"
                type="button"
                class="shrink-0 rounded-full px-3 py-1.5 text-xs font-semibold transition disabled:opacity-50"
                :class="
                  confirmingDelete === role.name
                    ? 'bg-red-600 text-white'
                    : 'text-neutral-500 hover:text-red-600 dark:hover:text-red-400'
                "
                :disabled="store.busy !== null"
                @click="remove(role.name)"
                @blur="confirmingDelete = null"
              >
                {{
                  store.busy === role.name
                    ? 'Deleting…'
                    : confirmingDelete === role.name
                      ? 'Click to confirm'
                      : 'Delete'
                }}
              </button>
            </div>

            <div
              v-if="connecting?.user === role.name"
              class="glass-divider border-t px-4 pt-3 pb-4"
            >
              <ConnectionSnippets
                :user="connecting.user"
                :password="connecting.password"
                :database="connecting.database"
              />
            </div>
          </li>
        </ul>
      </div>

      <div class="mt-4 flex items-center justify-between gap-3">
        <p class="text-xs text-neutral-400">
          Grants beyond this are a SQL client's job. Deleting a user that owns databases is refused.
        </p>
        <button
          type="button"
          class="glass-accent rounded-full px-4 py-2 text-sm font-semibold transition"
          @click="close"
        >
          Done
        </button>
      </div>
    </div>

    <NewPostgresUserModal v-if="adding" @close="closeNew" @created="onCreated" />
  </div>
</template>
