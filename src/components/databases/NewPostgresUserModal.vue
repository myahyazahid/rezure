<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { usePostgresUsersStore } from '@/stores/postgresUsers'
import ModalCloseButton from '@/components/common/ModalCloseButton.vue'

/**
 * The "new user" form, stacked over `PostgresUsersModal`. It creates the
 * user and hands back what was just set, so the list can show the connection
 * settings while the password is still known. Escape is the parent's to
 * handle — one window listener decides which modal it closes.
 */
const emit = defineEmits<{
  close: []
  created: [{ user: string; password: string | null; database: string | null }]
}>()

const store = usePostgresUsersStore()

type Preset = 'app' | 'framework' | 'odoo'

/** Starting points for the common cases; every field stays editable. */
const PRESETS: { id: Preset; label: string; hint: string }[] = [
  {
    id: 'app',
    label: 'App',
    hint: 'A user that owns one database - Laravel, Express, most apps.',
  },
  {
    id: 'framework',
    label: 'Django / Rails',
    hint: 'Owns its database and may create more, for the test runner.',
  },
  {
    id: 'odoo',
    label: 'Odoo',
    hint: 'Creates its own databases from the Odoo database manager.',
  },
]

const preset = ref<Preset>('app')
const name = ref('')
const password = ref('')
const canCreateDb = ref(false)
const withDatabase = ref(true)
const databaseName = ref('')
/** Typed until the user edits it — then it's theirs. */
const databaseTouched = ref(false)
/** So an error left over from the list (a refused delete) isn't shown here. */
const submitted = ref(false)

function applyPreset(id: Preset) {
  preset.value = id
  canCreateDb.value = id !== 'app'
  withDatabase.value = id !== 'odoo'
  if (id === 'odoo' && !name.value) name.value = 'odoo'
}

watch(name, (value) => {
  if (!databaseTouched.value) databaseName.value = value
})

/** A password worth keeping, so "no password" isn't the easy option. */
function generatePassword() {
  const alphabet = 'ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789'
  const bytes = crypto.getRandomValues(new Uint8Array(20))
  password.value = Array.from(bytes, (b) => alphabet[b % alphabet.length]).join('')
}

/** Mirrors `validate_identifier` and `postgres_roles::validate_role_name`
 *  — Rust enforces them; this only says so while typing. */
const NAME_PATTERN = /^[A-Za-z0-9_-]+$/

const nameError = computed(() => {
  const value = name.value.trim()
  if (!value) return null
  if (value.length > 64) return 'Keep it under 64 characters.'
  if (!NAME_PATTERN.test(value)) return 'Letters, digits, underscores and hyphens only.'
  if (value.toLowerCase().startsWith('pg_')) return 'pg_ is kept for PostgreSQL itself.'
  if (store.roles.some((r) => r.name === value)) return `"${value}" already exists.`
  return null
})
const databaseError = computed(() => {
  if (!withDatabase.value) return null
  const value = databaseName.value.trim()
  if (!value) return 'Name the database, or untick it.'
  if (!NAME_PATTERN.test(value)) return 'Letters, digits, underscores and hyphens only.'
  return null
})
const canCreate = computed(
  () => name.value.trim() !== '' && !nameError.value && !databaseError.value,
)

async function submit() {
  if (!canCreate.value) return
  const user = name.value.trim()
  const database = withDatabase.value ? databaseName.value.trim() : null
  const typed = password.value || null
  submitted.value = true
  const ok = await store.create({
    name: user,
    password: typed,
    canCreateDb: canCreateDb.value,
    database,
  })
  if (ok) emit('created', { user, password: typed, database })
}

function close() {
  if (store.busy === 'new') return
  emit('close')
}

onMounted(() => applyPreset('app'))

const FIELD_CLASS =
  'glass-inset mt-1 w-full rounded-xl px-3.5 py-2 font-mono text-sm text-neutral-900 outline-none focus:border-accent-400/70 dark:text-neutral-100'
</script>

<template>
  <!-- Teleported so the users modal's card doesn't clip it; z-55 sits above
       that modal (z-50) and under BusyOverlay (z-60). -->
  <Teleport to="body">
    <div class="glass-scrim fixed inset-0 z-55 flex items-center justify-center p-4">
      <form
        class="glass-strong relative flex max-h-[88vh] w-full max-w-lg flex-col rounded-3xl p-6"
        @submit.prevent="submit"
      >
        <ModalCloseButton :disabled="store.busy === 'new'" @click="close" />
        <h2 class="pr-8 text-xl font-bold tracking-tight">New PostgreSQL user</h2>
        <p class="mt-1 text-sm text-neutral-500">
          Pick what it's for - the options below follow, and stay editable.
        </p>

        <div class="-mx-1 mt-4 flex-1 overflow-y-auto px-1">
          <p class="text-[11px] font-semibold tracking-wide text-neutral-500 uppercase">For</p>
          <div class="mt-1.5 grid gap-2 sm:grid-cols-3">
            <button
              v-for="p in PRESETS"
              :key="p.id"
              type="button"
              class="rounded-xl px-3 py-2 text-left transition"
              :class="
                preset === p.id
                  ? 'glass-raised ring-2 ring-accent-500/40'
                  : 'glass-btn opacity-80 hover:opacity-100'
              "
              @click="applyPreset(p.id)"
            >
              <span class="block text-sm font-semibold">{{ p.label }}</span>
              <span class="mt-0.5 block text-[11px] leading-snug text-neutral-500">{{
                p.hint
              }}</span>
            </button>
          </div>

          <div class="mt-4 grid gap-3 sm:grid-cols-2">
            <label class="block text-xs font-medium text-neutral-500">
              User name
              <input
                v-model="name"
                type="text"
                placeholder="my_app"
                autofocus
                :class="FIELD_CLASS"
              />
              <span v-if="nameError" class="mt-1 block text-red-600 dark:text-red-400">{{
                nameError
              }}</span>
            </label>
            <label class="block text-xs font-medium text-neutral-500">
              Password
              <div class="relative">
                <input
                  v-model="password"
                  type="text"
                  placeholder="optional"
                  autocomplete="off"
                  spellcheck="false"
                  :class="[FIELD_CLASS, 'pr-20']"
                />
                <button
                  type="button"
                  class="absolute top-1/2 right-1.5 mt-0.5 -translate-y-1/2 rounded-lg px-2 py-1 text-[11px] font-semibold text-accent-600 transition hover:bg-accent-500/10 dark:text-accent-400"
                  @click="generatePassword"
                >
                  Generate
                </button>
              </div>
            </label>
          </div>

          <label class="mt-4 flex items-center gap-2.5">
            <input v-model="withDatabase" type="checkbox" class="accent-accent-600" />
            <span class="text-sm text-neutral-700 dark:text-neutral-200"
              >Create a database it owns</span
            >
          </label>
          <input
            v-if="withDatabase"
            v-model="databaseName"
            type="text"
            placeholder="my_app"
            :class="[FIELD_CLASS, 'ml-6.5 w-[calc(100%-1.625rem)]']"
            @input="databaseTouched = true"
          />
          <p v-if="databaseError" class="mt-1 ml-6.5 text-xs text-red-600 dark:text-red-400">
            {{ databaseError }}
          </p>
          <label class="mt-2 flex items-center gap-2.5">
            <input v-model="canCreateDb" type="checkbox" class="accent-accent-600" />
            <span class="text-sm text-neutral-700 dark:text-neutral-200"
              >May create databases of its own</span
            >
          </label>

          <p
            v-if="submitted && store.error && store.busy === null"
            class="mt-3 text-xs text-red-600 dark:text-red-400"
          >
            {{ store.error }}
          </p>
        </div>

        <div class="mt-5 flex justify-end gap-2">
          <button
            type="button"
            class="rounded-full px-4 py-2 text-sm font-semibold text-neutral-600 dark:text-neutral-300"
            :disabled="store.busy === 'new'"
            @click="close"
          >
            Cancel
          </button>
          <button
            type="submit"
            class="glass-accent rounded-full px-4 py-2 text-sm font-semibold transition disabled:opacity-50"
            :disabled="!canCreate || store.busy !== null"
          >
            {{ store.busy === 'new' ? 'Creating…' : 'Create user' }}
          </button>
        </div>
      </form>
    </div>
  </Teleport>
</template>
