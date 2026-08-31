<script setup lang="ts">
import { ref } from 'vue'
import { open as openFileDialog } from '@tauri-apps/plugin-dialog'
import { useDbProfilesStore } from '@/stores/dbProfiles'
import { ENGINE_LABEL } from '@/types/dbProfile'
import type { DetectedDatadir } from '@/types/dbProfile'

const emit = defineEmits<{ close: [] }>()
const store = useDbProfilesStore()

/** Manual entry, for a datadir no scan found. */
const manual = ref(false)
const name = ref('')
const datadirPath = ref('')
const port = ref(3306)

async function pickFolder() {
  const picked = await openFileDialog({ directory: true, multiple: false })
  if (typeof picked === 'string') {
    datadirPath.value = picked
    // A sensible default the user can overwrite, taken from the folder name.
    if (!name.value) name.value = picked.split(/[\\/]/).filter(Boolean).pop() ?? ''
  }
}

async function adopt(found: DetectedDatadir) {
  const ok = await store.add({
    name: found.name,
    datadirPath: found.datadirPath,
    engine: found.engine,
    version: found.version,
    port: 3306,
    source: found.source,
    // Runs on the build that came with it — a datadir needs a binary of its
    // own major.minor, and that one is already on disk.
    binaryDir: found.binaryDir,
    // The config that build launches with — the datadir depends on it.
    defaultsFile: found.defaultsFile,
  })
  if (ok && store.detected.length === 0) emit('close')
}

async function addManual() {
  if (!datadirPath.value.trim()) return
  const ok = await store.add({
    name: name.value.trim() || 'Custom',
    datadirPath: datadirPath.value.trim(),
    version: '',
    port: port.value,
    source: 'custom',
  })
  if (ok) emit('close')
}
</script>

<template>
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
    @click.self="emit('close')"
  >
    <div
      class="max-h-[85vh] w-full max-w-lg overflow-y-auto rounded-2xl border border-neutral-200 bg-white p-6 shadow-2xl dark:border-neutral-700 dark:bg-neutral-900"
    >
      <h2 class="text-lg font-bold text-neutral-900 dark:text-neutral-100">Add data directory</h2>
      <p class="mt-1 text-sm text-neutral-500">
        Rezure runs one database server at a time and points it at the directory you pick. Your data
        is never copied, moved or converted.
      </p>

      <p v-if="store.error" class="mt-3 text-sm text-red-600 dark:text-red-400">
        {{ store.error }}
      </p>

      <template v-if="!manual">
        <p v-if="store.detecting" class="mt-5 text-sm text-neutral-500">Looking for other tools…</p>

        <div v-else-if="store.detected.length > 0" class="mt-5 space-y-2">
          <div
            v-for="found in store.detected"
            :key="found.datadirPath"
            class="rounded-xl border border-neutral-200 p-3.5 dark:border-neutral-700"
          >
            <div class="flex items-start justify-between gap-3">
              <div class="min-w-0">
                <p class="truncate font-semibold text-neutral-900 dark:text-neutral-100">
                  {{ found.name }}
                </p>
                <p class="mt-0.5 text-xs text-neutral-500">
                  {{ ENGINE_LABEL[found.engine] }} {{ found.version }}
                </p>
                <p
                  class="mt-1 truncate font-mono text-xs text-neutral-400"
                  :title="found.datadirPath"
                >
                  {{ found.datadirPath }}
                </p>
                <!-- Worth stating: without its own build, this datadir
                     can't be opened at all. -->
                <p v-if="!found.binaryDir" class="mt-1 text-xs text-amber-600 dark:text-amber-400">
                  No server binary found beside it — you'll need a matching
                  {{ ENGINE_LABEL[found.engine] }} {{ found.version }} build.
                </p>
              </div>
              <button
                type="button"
                class="shrink-0 rounded-full bg-red-600 px-4 py-2 text-sm font-semibold text-white transition hover:bg-red-500 disabled:opacity-50"
                :disabled="store.adding"
                @click="adopt(found)"
              >
                Add
              </button>
            </div>
          </div>
        </div>

        <p v-else class="mt-5 text-sm text-neutral-500">
          No other database directories found. Laragon and XAMPP are checked in their default
          locations.
        </p>

        <button
          type="button"
          class="mt-5 text-sm font-semibold text-red-600 underline dark:text-red-400"
          @click="manual = true"
        >
          Point at a folder myself
        </button>
      </template>

      <template v-else>
        <div class="mt-5 space-y-4">
          <div>
            <label class="block text-sm font-semibold text-neutral-700 dark:text-neutral-200">
              Name
            </label>
            <input
              v-model="name"
              type="text"
              placeholder="My data"
              class="mt-1.5 w-full rounded-lg border border-neutral-200 bg-white px-3 py-2 text-sm dark:border-neutral-700 dark:bg-neutral-800 dark:text-neutral-100"
            />
          </div>

          <div>
            <label class="block text-sm font-semibold text-neutral-700 dark:text-neutral-200">
              Data directory
            </label>
            <div class="mt-1.5 flex gap-2">
              <input
                v-model="datadirPath"
                type="text"
                placeholder="C:\path\to\data"
                class="min-w-0 flex-1 rounded-lg border border-neutral-200 bg-white px-3 py-2 font-mono text-xs dark:border-neutral-700 dark:bg-neutral-800 dark:text-neutral-100"
              />
              <button
                type="button"
                class="shrink-0 rounded-lg border border-neutral-200 px-3 py-2 text-sm font-semibold dark:border-neutral-700 dark:text-neutral-200"
                @click="pickFolder"
              >
                Browse
              </button>
            </div>
            <p class="mt-1 text-xs text-neutral-400">
              Rezure reads the engine from the folder itself, so there's nothing to pick wrong.
            </p>
          </div>

          <div>
            <label class="block text-sm font-semibold text-neutral-700 dark:text-neutral-200">
              Port
            </label>
            <input
              v-model.number="port"
              type="number"
              min="1"
              max="65535"
              class="mt-1.5 w-28 rounded-lg border border-neutral-200 bg-white px-3 py-2 text-sm dark:border-neutral-700 dark:bg-neutral-800 dark:text-neutral-100"
            />
          </div>
        </div>

        <div class="mt-6 flex justify-end gap-2">
          <button
            type="button"
            class="rounded-full px-4 py-2 text-sm font-semibold text-neutral-600 dark:text-neutral-300"
            @click="manual = false"
          >
            Back
          </button>
          <button
            type="button"
            class="rounded-full bg-red-600 px-5 py-2 text-sm font-semibold text-white transition hover:bg-red-500 disabled:opacity-50"
            :disabled="store.adding || !datadirPath.trim()"
            @click="addManual"
          >
            {{ store.adding ? 'Adding…' : 'Add' }}
          </button>
        </div>
      </template>

      <button
        v-if="!manual"
        type="button"
        class="mt-6 w-full rounded-full border border-neutral-200 py-2 text-sm font-semibold text-neutral-600 dark:border-neutral-700 dark:text-neutral-300"
        @click="emit('close')"
      >
        Done
      </button>
    </div>
  </div>
</template>
