<script setup lang="ts">
import { computed, ref } from 'vue'
import { useDbConnectionsStore } from '@/stores/dbConnections'
import type { DbConnectionStatus } from '@/types/dbConnection'

const props = defineProps<{ connection: DbConnectionStatus }>()
const emit = defineEmits<{ close: []; unlocked: [] }>()

const store = useDbConnectionsStore()

const password = ref('')
/** Defaults to what the connection was saved with, so a user who chose not
 *  to persist the password isn't quietly opted into persisting it now. */
const save = ref(props.connection.savePassword)

const canSubmit = computed(() => password.value.length > 0 && !store.saving)

async function submit() {
  if (!canSubmit.value) return
  const ok = await store.unlock(props.connection.id, password.value, save.value)
  if (ok) {
    emit('unlocked')
    emit('close')
  }
}
</script>

<template>
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
    @click.self="emit('close')"
  >
    <div class="w-full max-w-sm rounded-2xl bg-white p-6 shadow-2xl dark:bg-neutral-900">
      <h2 class="text-lg font-bold text-neutral-900 dark:text-neutral-100">
        Password for {{ props.connection.name }}
      </h2>
      <p class="mt-1 font-mono text-xs text-neutral-500">
        {{ props.connection.user }}@{{ props.connection.host }}:{{ props.connection.port }}
      </p>

      <input
        v-model="password"
        type="password"
        placeholder="••••••••"
        autofocus
        class="mt-4 w-full rounded-xl border border-neutral-200 bg-white px-3.5 py-2.5 text-sm text-neutral-900 outline-none transition focus:border-red-400 dark:border-neutral-700 dark:bg-neutral-950 dark:text-neutral-100"
        @keyup.enter="submit"
      />

      <label class="mt-3 flex items-center gap-2.5">
        <input v-model="save" type="checkbox" class="accent-red-600" />
        <span class="text-sm text-neutral-600 dark:text-neutral-300">
          Save it in Windows Credential Manager
        </span>
      </label>

      <p v-if="store.error" class="mt-3 text-sm text-red-600 dark:text-red-400">
        {{ store.error }}
      </p>

      <div class="mt-5 flex justify-end gap-2">
        <button
          type="button"
          class="rounded-full px-4 py-2.5 text-sm font-semibold text-neutral-500 transition hover:text-neutral-800 dark:hover:text-neutral-200"
          @click="emit('close')"
        >
          Cancel
        </button>
        <button
          type="button"
          class="rounded-full bg-red-600 px-5 py-2.5 text-sm font-semibold text-white shadow-lg shadow-red-500/40 transition hover:bg-red-500 disabled:cursor-not-allowed disabled:opacity-50"
          :disabled="!canSubmit"
          @click="submit"
        >
          {{ store.saving ? 'Unlocking…' : 'Unlock' }}
        </button>
      </div>
    </div>
  </div>
</template>
