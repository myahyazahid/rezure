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
    .replace(/\.sql$/i, '')
    .replace(/-\d{8}-\d{6}$/, '') // strip Rezure's own export timestamp
    .replace(/[^A-Za-z0-9_-]/g, '_')
    .slice(0, 64)
}

const name = ref(suggestedName(props.file))

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

const canImport = computed(() => name.value.trim().length > 0 && !nameError.value)

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
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4 backdrop-blur-sm"
    @click.self="close"
  >
    <div class="w-full max-w-md rounded-3xl bg-white p-6 shadow-2xl dark:bg-neutral-900">
      <h2 class="text-xl font-bold tracking-tight">Import .sql</h2>
      <p class="mt-0.5 truncate font-mono text-xs text-neutral-500" :title="props.file">
        {{ props.file }}
      </p>

      <label class="mt-5 block text-xs font-medium text-neutral-500">Import into</label>
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

      <p
        v-else-if="existing"
        class="mt-2 rounded-xl bg-amber-50 px-3 py-2 text-xs text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
      >
        <strong>{{ name.trim() }}</strong> already exists. A dump usually contains
        <code>DROP TABLE</code> statements, so tables it defines will be replaced.
      </p>
      <p v-else class="mt-2 text-xs text-neutral-500">
        <strong>{{ name.trim() || '…' }}</strong> doesn't exist yet — it will be created.
      </p>

      <p v-if="store.importError" class="mt-3 text-sm text-red-600 dark:text-red-400">
        {{ store.importError }}
      </p>

      <div class="mt-5 flex justify-end gap-2">
        <button
          type="button"
          class="rounded-full border border-neutral-200 bg-white/70 px-5 py-2.5 text-sm font-semibold text-neutral-700 transition hover:bg-white disabled:opacity-50 dark:border-neutral-700 dark:bg-neutral-900/60 dark:text-neutral-200 dark:hover:bg-neutral-800"
          :disabled="store.importing"
          @click="close"
        >
          Cancel
        </button>
        <button
          type="button"
          class="rounded-full bg-red-600 px-5 py-2.5 text-sm font-semibold text-white shadow-lg shadow-red-500/40 transition hover:bg-red-500 disabled:cursor-not-allowed disabled:opacity-50"
          :disabled="!canImport || store.importing"
          @click="submit"
        >
          {{ store.importing ? 'Importing…' : 'Import' }}
        </button>
      </div>
    </div>
  </div>
</template>
