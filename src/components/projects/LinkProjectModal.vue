<script setup lang="ts">
import { ref } from 'vue'
import { open as openFileDialog } from '@tauri-apps/plugin-dialog'
import { useProjectsStore } from '@/stores/projects'
import type { LinkPreview } from '@/types/project'

const emit = defineEmits<{ close: [] }>()
const store = useProjectsStore()

const preview = ref<LinkPreview | null>(null)
const name = ref('')
const domain = ref('')
const pathError = ref<string | null>(null)
const checking = ref(false)

function errorMessage(e: unknown): string {
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  return 'Something went wrong.'
}

/**
 * Picks a folder and asks the backend what linking it would produce — the
 * name, stack, docroot and domain — so the user sees the outcome and any
 * refusal before committing, not after.
 */
async function pickFolder() {
  const picked = await openFileDialog({ directory: true, multiple: false })
  if (typeof picked !== 'string') return

  checking.value = true
  pathError.value = null
  preview.value = null
  try {
    const result = await store.previewLink(picked)
    preview.value = result
    name.value = result.name
    domain.value = result.domain
  } catch (e) {
    pathError.value = errorMessage(e)
  } finally {
    checking.value = false
  }
}

async function confirm() {
  if (!preview.value) return
  const ok = await store.linkProject(preview.value.path, name.value, domain.value)
  if (ok) emit('close')
}
</script>

<template>
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
    @click.self="emit('close')"
  >
    <div
      class="w-full max-w-lg rounded-2xl border border-neutral-200 bg-white p-6 shadow-2xl dark:border-neutral-700 dark:bg-neutral-900"
    >
      <h2 class="text-lg font-bold text-neutral-900 dark:text-neutral-100">Add existing folder</h2>
      <p class="mt-1 text-sm text-neutral-500">
        Serve a project from wherever it already lives. Rezure records the path and nothing else —
        the folder isn't copied, moved, or written to.
      </p>

      <div class="mt-5">
        <button
          type="button"
          class="flex w-full items-center justify-center gap-2 rounded-xl border border-dashed border-neutral-300 py-6 text-sm font-semibold text-neutral-600 transition hover:border-red-400 hover:text-red-600 dark:border-neutral-600 dark:text-neutral-300 dark:hover:border-red-500 dark:hover:text-red-400"
          :disabled="checking"
          @click="pickFolder"
        >
          <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            class="h-5 w-5"
          >
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"
            />
          </svg>
          {{ checking ? 'Checking…' : preview ? 'Choose a different folder' : 'Choose a folder' }}
        </button>
      </div>

      <!-- A refusal is explained here rather than after the fact: the
           backend won't serve a drive root, a system folder, or something
           already covered by the www scan. -->
      <p v-if="pathError" class="mt-3 text-sm text-red-600 dark:text-red-400">{{ pathError }}</p>
      <p v-if="store.linkError" class="mt-3 text-sm text-red-600 dark:text-red-400">
        {{ store.linkError }}
      </p>

      <template v-if="preview">
        <div
          class="mt-4 rounded-xl border border-neutral-200 bg-neutral-50 p-3.5 dark:border-neutral-700 dark:bg-neutral-800/40"
        >
          <p class="truncate font-mono text-xs text-neutral-500" :title="preview.path">
            {{ preview.path }}
          </p>
          <p class="mt-1.5 text-xs text-neutral-500">
            Detected as
            <strong class="text-neutral-700 dark:text-neutral-200">{{ preview.stack }}</strong>
            <!-- Worth showing: Laravel is served from public/, so the folder
                 nginx uses isn't always the one that was picked. -->
            <template v-if="preview.docroot !== preview.path">
              · served from
              <span class="font-mono">{{ preview.docroot.split(/[\\/]/).pop() }}/</span>
            </template>
          </p>
          <p
            v-if="preview.stack === 'Unknown'"
            class="mt-1.5 text-xs text-amber-600 dark:text-amber-400"
          >
            No framework markers found here — it'll still be served as static files.
          </p>
        </div>

        <div class="mt-4 grid grid-cols-2 gap-3">
          <div>
            <label class="block text-sm font-semibold text-neutral-700 dark:text-neutral-200">
              Name
            </label>
            <input
              v-model="name"
              type="text"
              class="mt-1.5 w-full rounded-lg border border-neutral-200 bg-white px-3 py-2 text-sm dark:border-neutral-700 dark:bg-neutral-800 dark:text-neutral-100"
            />
          </div>
          <div>
            <label class="block text-sm font-semibold text-neutral-700 dark:text-neutral-200">
              Domain
            </label>
            <input
              v-model="domain"
              type="text"
              class="mt-1.5 w-full rounded-lg border border-neutral-200 bg-white px-3 py-2 font-mono text-sm dark:border-neutral-700 dark:bg-neutral-800 dark:text-neutral-100"
            />
          </div>
        </div>
        <p v-if="preview.domainAdjusted" class="mt-1.5 text-xs text-neutral-500">
          The obvious name was already taken by another project, so this one was numbered.
        </p>
      </template>

      <div class="mt-6 flex justify-end gap-2">
        <button
          type="button"
          class="rounded-full px-4 py-2 text-sm font-semibold text-neutral-600 dark:text-neutral-300"
          @click="emit('close')"
        >
          Cancel
        </button>
        <button
          type="button"
          class="rounded-full bg-red-600 px-5 py-2 text-sm font-semibold text-white transition hover:bg-red-500 disabled:opacity-50"
          :disabled="!preview || store.linking || !name.trim() || !domain.trim()"
          @click="confirm"
        >
          {{ store.linking ? 'Adding…' : 'Add project' }}
        </button>
      </div>
    </div>
  </div>
</template>
