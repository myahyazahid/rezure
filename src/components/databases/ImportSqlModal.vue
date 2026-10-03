<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useDatabasesStore } from '@/stores/databases'

const props = defineProps<{ file: string }>()
const emit = defineEmits<{ close: [] }>()

const store = useDatabasesStore()

const NAME_PATTERN = /^[A-Za-z0-9_-]+$/

/** A dump is usually named after the database it came from, so that's the
 *  best first guess for where it should go back in. */
function suggestedName(path: string) {
  const base = path.split(/[\\/]/).pop() ?? ''
  return base
    .replace(/\.(sql|bak)$/i, '')
    .replace(/-\d{8}-\d{6}$/, '') // strip Rezure's own export timestamp
    .replace(/[^A-Za-z0-9_-]/g, '_')
    .slice(0, 64)
}

const name = ref(suggestedName(props.file))

/** A SQL Server backup is restored whole, not run statement by statement —
 *  which changes what "already exists" means. */
const isBackup = computed(() => /\.bak$/i.test(props.file))

const existing = computed(() =>
  store.databases.some((db) => db.name.toLowerCase() === name.value.trim().toLowerCase()),
)

const nameError = computed(() => {
  const value = name.value.trim()
  if (!value) return null
  if (value.length > 64) return 'Keep it under 64 characters.'
  if (!NAME_PATTERN.test(value)) {
    return 'Letters, digits, underscores and hyphens only — no spaces.'
  }
  return null
})

/** True when the dump is about to be loaded into a server Rezure doesn't
 *  run. Everything below that reads `remote` exists because of it. */
const remote = computed(() => store.server?.remote === true)

/** A `pg_dump` script has no DROP statements unless it was taken with
 *  `--clean` — Rezure's own exports aren't — so into an existing database it
 *  stops at the first table that's already there. */
const isPostgres = computed(() => store.server?.kind === 'postgres')

/** Typed back by the user before a remote import runs.
 *
 *  A local import lands in a throwaway dev database; the same click against
 *  staging replaces tables other people are using, and it can't be undone
 *  from here. The dialog asks for the database name rather than a plain
 *  "are you sure" because retyping it is the step that makes the user read
 *  which database they picked. */
const confirmation = ref('')

const confirmed = computed(() => !remote.value || confirmation.value.trim() === name.value.trim())

const canImport = computed(
  () => name.value.trim().length > 0 && !nameError.value && confirmed.value,
)

function close() {
  if (store.importing) return
  emit('close')
}

async function submit() {
  if (!canImport.value) return
  const ok = await store.importSql(name.value.trim(), props.file)
  if (ok) close()
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') close()
}

onMounted(() => window.addEventListener('keydown', onKeydown))
onUnmounted(() => window.removeEventListener('keydown', onKeydown))

watch(name, () => {
  store.importError = null
})
</script>

<template>
  <div
    class="glass-scrim fixed inset-0 z-50 flex items-center justify-center p-4"
    @click.self="close"
  >
    <div class="glass-strong w-full max-w-md rounded-3xl p-6">
      <h2 class="text-xl font-bold tracking-tight">
        {{ isBackup ? 'Restore .bak' : 'Import .sql' }}
      </h2>
      <p class="mt-0.5 truncate font-mono text-xs text-neutral-500" :title="props.file">
        {{ props.file }}
      </p>

      <label class="mt-5 block text-xs font-medium text-neutral-500">Import into</label>
      <input
        v-model="name"
        type="text"
        placeholder="my_app"
        autofocus
        class="glass-inset mt-1 w-full rounded-xl px-3.5 py-2.5 font-mono text-sm text-neutral-900 outline-none dark:text-neutral-100"
        :class="nameError ? 'border-red-400 focus:border-red-500' : 'focus:border-accent-400/70'"
        @keyup.enter="submit"
      />
      <p v-if="nameError" class="mt-1.5 text-xs text-red-600 dark:text-red-400">{{ nameError }}</p>

      <p
        v-else-if="existing && isBackup"
        class="mt-2 rounded-xl bg-amber-100/50 px-3 py-2 text-xs text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
      >
        <strong>{{ name.trim() }}</strong> already exists. Restoring replaces it entirely — every
        table and row in it now is gone afterwards.
      </p>
      <p
        v-else-if="existing && isPostgres"
        class="mt-2 rounded-xl bg-amber-100/50 px-3 py-2 text-xs text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
      >
        <strong>{{ name.trim() }}</strong> already exists. A PostgreSQL dump usually creates its
        tables without dropping them first, so the import stops at the first one that's already
        there — import into a new name to be safe.
      </p>
      <p
        v-else-if="existing"
        class="mt-2 rounded-xl bg-amber-100/50 px-3 py-2 text-xs text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
      >
        <strong>{{ name.trim() }}</strong> already exists. A dump usually contains
        <code>DROP TABLE</code> statements, so tables it defines will be replaced.
      </p>
      <p v-else class="mt-2 text-xs text-neutral-500">
        <strong>{{ name.trim() || '…' }}</strong> doesn't exist yet — it will be created.
      </p>

      <!-- The second gate, and the only one that names the server. The first
           is the backend's own read-only check, which a connection marked
           read-only fails before anything reaches it. -->
      <div
        v-if="remote"
        class="mt-4 rounded-xl border border-amber-400/50 bg-amber-100/50 px-3.5 py-3 dark:border-amber-500/30 dark:bg-amber-500/10"
      >
        <p class="text-xs text-amber-900 dark:text-amber-200">
          This writes to <strong>{{ store.server?.label }}</strong> at
          <span class="font-mono">{{ store.server?.host }}</span> — a server Rezure doesn't run.
          Type <strong class="font-mono">{{ name.trim() || '…' }}</strong> to confirm.
        </p>
        <input
          v-model="confirmation"
          type="text"
          :placeholder="name.trim()"
          class="glass-inset mt-2 w-full rounded-lg border-amber-400/50 px-3 py-2 font-mono text-sm outline-none focus:border-amber-500 dark:border-amber-500/30"
        />
      </div>

      <p v-if="store.importError" class="mt-3 text-sm text-red-600 dark:text-red-400">
        {{ store.importError }}
      </p>

      <div class="mt-5 flex justify-end gap-2">
        <button
          type="button"
          class="glass-btn rounded-full px-4 py-2 text-sm font-semibold text-neutral-700 transition disabled:opacity-50 dark:text-neutral-200"
          :disabled="store.importing"
          @click="close"
        >
          Cancel
        </button>
        <button
          type="button"
          class="glass-accent rounded-full px-4 py-2 text-sm font-semibold transition disabled:cursor-not-allowed disabled:opacity-50"
          :disabled="!canImport || store.importing"
          @click="submit"
        >
          <template v-if="isBackup">{{ store.importing ? 'Restoring…' : 'Restore' }}</template>
          <template v-else>{{ store.importing ? 'Importing…' : 'Import' }}</template>
        </button>
      </div>
    </div>
  </div>
</template>
