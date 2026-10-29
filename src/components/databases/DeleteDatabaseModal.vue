<script setup lang="ts">
import { onMounted, onUnmounted, watch } from 'vue'
import { useDatabasesStore } from '@/stores/databases'
import { useTypedConfirm } from '@/composables/useTypedConfirm'
import type { DatabaseInfo } from '@/types/database'

const props = defineProps<{ database: DatabaseInfo; size: string }>()
const emit = defineEmits<{ close: [] }>()

const store = useDatabasesStore()
const { typed, confirmed } = useTypedConfirm()

function close() {
  if (store.dropping) return
  store.dropError = null
  emit('close')
}

async function submit() {
  if (!confirmed.value || store.dropping) return
  const ok = await store.dropDatabase(props.database.name)
  if (ok) close()
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') close()
}

onMounted(() => window.addEventListener('keydown', onKeydown))
onUnmounted(() => window.removeEventListener('keydown', onKeydown))

watch(typed, () => {
  store.dropError = null
})
</script>

<template>
  <div
    class="glass-scrim fixed inset-0 z-50 flex items-center justify-center p-4"
    @click.self="close"
  >
    <div
      class="glass-strong w-full max-w-md rounded-3xl p-6"
      role="alertdialog"
      aria-labelledby="delete-db-title"
    >
      <div class="flex items-start gap-3">
        <span
          class="flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-red-500/15 text-red-600 dark:text-red-400"
        >
          <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            class="h-5 w-5"
            aria-hidden="true"
          >
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              d="M4 7h16M10 11v6M14 11v6M5 7l1 12a2 2 0 0 0 2 2h8a2 2 0 0 0 2-2l1-12M9 7V4h6v3"
            />
          </svg>
        </span>
        <div class="min-w-0">
          <h2 id="delete-db-title" class="text-xl font-bold tracking-tight">Delete database</h2>
          <p class="mt-0.5 truncate font-mono text-sm text-neutral-600 dark:text-neutral-300">
            {{ database.name }}
          </p>
        </div>
      </div>

      <p class="mt-4 text-sm text-neutral-600 dark:text-neutral-300">
        This permanently deletes
        <span class="font-mono font-semibold text-neutral-900 dark:text-neutral-100">{{
          database.name
        }}</span>
        and all {{ database.tableCount }} tables in it ({{ size }}) from
        {{ store.server?.label || 'the local server' }}. There is no undo - export it first if you
        might need it again.
      </p>

      <p
        v-if="database.usedBy"
        class="mt-3 rounded-xl border border-amber-300/60 bg-amber-100/50 px-3 py-2 text-xs text-amber-900 dark:border-amber-500/25 dark:bg-amber-500/10 dark:text-amber-200"
      >
        <span class="font-mono font-semibold">{{ database.usedBy }}</span> uses this database. It
        will stop working until the database is created again.
      </p>

      <label for="delete-db-confirm" class="mt-5 block text-xs font-medium text-neutral-500">
        Type <span class="font-mono font-semibold text-red-600 dark:text-red-400">CONFIRM</span>
        to delete
      </label>
      <input
        id="delete-db-confirm"
        v-model="typed"
        type="text"
        placeholder="CONFIRM"
        autocomplete="off"
        spellcheck="false"
        autofocus
        class="glass-inset mt-1 w-full rounded-xl px-3.5 py-2.5 font-mono text-sm text-neutral-900 outline-none focus:border-red-400/70 dark:text-neutral-100"
        @keyup.enter="submit"
      />

      <p v-if="store.dropError" class="mt-3 text-sm text-red-600 dark:text-red-400">
        {{ store.dropError }}
      </p>

      <div class="mt-5 flex justify-end gap-2">
        <button
          type="button"
          class="glass-btn rounded-full px-4 py-2 text-sm font-semibold text-neutral-700 transition disabled:opacity-50 dark:text-neutral-200"
          :disabled="store.dropping"
          @click="close"
        >
          Cancel
        </button>
        <button
          type="button"
          class="glass-accent rounded-full px-4 py-2 text-sm font-semibold text-red-600 transition disabled:cursor-not-allowed disabled:opacity-50 dark:text-red-400"
          :disabled="!confirmed || store.dropping"
          @click="submit"
        >
          {{ store.dropping ? 'Deleting…' : 'Delete database' }}
        </button>
      </div>
    </div>
  </div>
</template>
