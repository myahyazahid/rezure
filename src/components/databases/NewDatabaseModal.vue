<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useDatabasesStore } from '@/stores/databases'

const emit = defineEmits<{ close: [] }>()

const store = useDatabasesStore()

const DEFAULT_COLLATION = 'utf8mb4_unicode_ci'

const name = ref('')
const collation = ref(DEFAULT_COLLATION)

/** Mirrors `validate_identifier` in `services/database.rs` — the Rust side
 *  is the one that actually enforces this; checking here just means the
 *  user finds out while typing instead of after a round trip. */
const NAME_PATTERN = /^[A-Za-z0-9_-]+$/

const nameError = computed(() => {
  const value = name.value.trim()
  if (!value) return null
  if (value.length > 64) return 'Keep it under 64 characters.'
  if (!NAME_PATTERN.test(value)) {
    return 'Letters, digits, underscores and hyphens only — no spaces.'
  }
  if (store.databases.some((db) => db.name.toLowerCase() === value.toLowerCase())) {
    return `A database named "${value}" already exists.`
  }
  return null
})

const canCreate = computed(() => name.value.trim().length > 0 && !nameError.value)

function close() {
  if (store.creating) return
  emit('close')
}

async function submit() {
  if (!canCreate.value) return
  const ok = await store.createDatabase(name.value.trim(), collation.value)
  if (ok) close()
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') close()
}

onMounted(() => {
  window.addEventListener('keydown', onKeydown)
  if (store.collations.length === 0) store.fetchCollations()
})
onUnmounted(() => window.removeEventListener('keydown', onKeydown))

watch(name, () => {
  store.createError = null
})
</script>

<template>
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4 backdrop-blur-sm"
    @click.self="close"
  >
    <div class="w-full max-w-md rounded-3xl bg-white p-6 shadow-2xl dark:bg-neutral-900">
      <h2 class="text-xl font-bold tracking-tight">New database</h2>
      <p class="mt-0.5 text-sm text-neutral-500">
        Created on Rezure's MariaDB — empty, with no tables.
      </p>

      <label class="mt-5 block text-xs font-medium text-neutral-500">Database name</label>
      <input
        v-model="name"
        type="text"
        placeholder="my_app"
        autofocus
        class="mt-1 w-full rounded-xl border bg-white px-3.5 py-2.5 font-mono text-sm text-neutral-900 outline-none dark:bg-neutral-950 dark:text-neutral-100"
        :class="
          nameError
            ? 'border-red-400 focus:border-red-500'
            : 'border-neutral-200 focus:border-red-400 dark:border-neutral-700'
        "
        @keyup.enter="submit"
      />
      <p v-if="nameError" class="mt-1.5 text-xs text-red-600 dark:text-red-400">{{ nameError }}</p>

      <label class="mt-4 block text-xs font-medium text-neutral-500">Collation</label>
      <select
        v-model="collation"
        class="mt-1 w-full rounded-xl border border-neutral-200 bg-white px-3.5 py-2.5 font-mono text-sm text-neutral-900 outline-none focus:border-red-400 dark:border-neutral-700 dark:bg-neutral-950 dark:text-neutral-100"
      >
        <!-- The server's real list, once it has loaded; until then the
             default is still a valid choice on its own. -->
        <option v-if="store.collations.length === 0" :value="DEFAULT_COLLATION">
          {{ DEFAULT_COLLATION }}
        </option>
        <option v-for="option in store.collations" :key="option" :value="option">
          {{ option }}
        </option>
      </select>

      <p v-if="store.createError" class="mt-3 text-sm text-red-600 dark:text-red-400">
        {{ store.createError }}
      </p>

      <div class="mt-5 flex justify-end gap-2">
        <button
          type="button"
          class="rounded-full border border-neutral-200 bg-white/70 px-5 py-2.5 text-sm font-semibold text-neutral-700 transition hover:bg-white disabled:opacity-50 dark:border-neutral-700 dark:bg-neutral-900/60 dark:text-neutral-200 dark:hover:bg-neutral-800"
          :disabled="store.creating"
          @click="close"
        >
          Cancel
        </button>
        <button
          type="button"
          class="rounded-full bg-red-600 px-5 py-2.5 text-sm font-semibold text-white shadow-lg shadow-red-500/40 transition hover:bg-red-500 disabled:cursor-not-allowed disabled:opacity-50"
          :disabled="!canCreate || store.creating"
          @click="submit"
        >
          {{ store.creating ? 'Creating…' : 'Create database' }}
        </button>
      </div>
    </div>
  </div>
</template>
