<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { open as openFileDialog } from '@tauri-apps/plugin-dialog'
import { useProjectsStore } from '@/stores/projects'
import { servedFolderLabel } from '@/composables/servedFolder'
import FrameworkPicker from '@/components/projects/FrameworkPicker.vue'
import type { FrameworkChoice, LinkPreview } from '@/types/project'

const emit = defineEmits<{ close: [] }>()
const store = useProjectsStore()

const preview = ref<LinkPreview | null>(null)
const name = ref('')
const domain = ref('')
const pathError = ref<string | null>(null)
const checking = ref(false)

/** The user's correction of what was detected — `null` keeps detection. */
const choice = ref<FrameworkChoice>(null)
const choiceError = ref<string | null>(null)
/** "Custom folder…" picked with no folder typed yet. */
const choicePending = ref(false)

onMounted(() => {
  if (store.frameworkCatalog.frameworks.length === 0) store.fetchFrameworks()
})

function errorMessage(e: unknown): string {
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  return 'Something went wrong.'
}

/**
 * Asks the backend what linking `path` would produce — the name, framework,
 * docroot and domain — so the user sees the outcome and any refusal before
 * committing, not after. Starts over from detection for a new folder.
 */
async function previewFolder(path: string) {
  checking.value = true
  pathError.value = null
  choiceError.value = null
  preview.value = null
  choice.value = null
  try {
    const result = await store.previewLink(path)
    preview.value = result
    name.value = result.name
    domain.value = result.domain
  } catch (e) {
    pathError.value = errorMessage(e)
  } finally {
    checking.value = false
  }
}

async function pickFolder() {
  const picked = await openFileDialog({ directory: true, multiple: false })
  if (typeof picked !== 'string') return
  await previewFolder(picked)
}

// A different framework means a different docroot, and only Rust knows
// which — so every change is previewed again rather than guessed here. The
// name and domain the user may have edited are left as they are.
watch(choice, async (next, previous) => {
  if (!preview.value || JSON.stringify(next) === JSON.stringify(previous)) return
  choiceError.value = null
  try {
    preview.value = await store.previewLink(preview.value.path, next)
  } catch (e) {
    choiceError.value = errorMessage(e)
  }
})

async function confirm() {
  if (!preview.value || choiceError.value || choicePending.value) return
  const ok = await store.linkProject(preview.value.path, name.value, domain.value, choice.value)
  if (ok) emit('close')
}
</script>

<template>
  <div
    class="glass-scrim fixed inset-0 z-50 flex items-center justify-center p-4"
    @click.self="emit('close')"
  >
    <div class="glass-strong w-full max-w-lg rounded-2xl p-6">
      <h2 class="text-lg font-bold text-neutral-900 dark:text-neutral-100">Add existing folder</h2>
      <p class="mt-1 text-sm text-neutral-500">
        Serve a project from wherever it already lives. Rezure records the path and nothing else -
        the folder isn't copied, moved, or written to.
      </p>

      <div class="mt-5">
        <button
          type="button"
          class="flex w-full items-center justify-center gap-2 rounded-xl border border-dashed border-neutral-900/20 py-6 text-sm font-semibold text-neutral-600 transition hover:border-accent-400 hover:text-accent-600 dark:border-white/20 dark:text-neutral-300 dark:hover:border-accent-500 dark:hover:text-accent-400"
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
        <!-- Picking a project's public/ folder is the usual workaround when
             detection was wrong. It serves, but the project's folder and
             terminal buttons would open public/ — so offer the project. -->
        <div
          v-if="preview.suggestion"
          class="mt-4 rounded-xl bg-amber-100/50 px-3.5 py-2.5 text-sm text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
        >
          This looks like the
          <span class="font-mono">{{ preview.suggestion.folder }}/</span>
          folder of a {{ preview.suggestion.framework }} project. Rezure serves that folder on its
          own when you add the project itself.
          <button
            type="button"
            class="mt-2 block truncate font-semibold underline underline-offset-2"
            :title="preview.suggestion.path"
            @click="previewFolder(preview.suggestion.path)"
          >
            Add {{ preview.suggestion.path.split(/[\\/]/).pop() }} instead
          </button>
        </div>

        <div class="glass-inset mt-4 rounded-xl p-3.5">
          <p class="truncate font-mono text-xs text-neutral-500" :title="preview.path">
            {{ preview.path }}
          </p>
          <!-- Worth showing: the folder nginx uses isn't always the one that
               was picked (Laravel and CodeIgniter 4 serve public/). From the
               preview, not the picker: a docroot that isn't there falls back
               to the folder itself, and only Rust knows that. -->
          <p v-if="preview.serve === 'proxy'" class="mt-1.5 text-xs text-neutral-500">
            Served through its dev server - Rezure gives it a port and starts it when you click
            Open.
          </p>
          <p v-else class="mt-1.5 text-xs text-neutral-500">
            Served from
            <span class="font-mono text-neutral-700 dark:text-neutral-200">{{
              servedFolderLabel(preview.path, preview.docroot)
            }}</span>
          </p>
          <p
            v-if="preview.framework === 'unknown'"
            class="mt-1.5 text-xs text-amber-600 dark:text-amber-400"
          >
            No framework markers found here - it'll be served from the folder itself. Pick the
            framework below if you know it.
          </p>
        </div>

        <div class="mt-4">
          <label class="block text-sm font-semibold text-neutral-700 dark:text-neutral-200">
            Framework
          </label>
          <FrameworkPicker
            v-model="choice"
            class="mt-1.5"
            :detected="preview.detectedFramework"
            @pending="choicePending = $event"
          />
          <p v-if="choiceError" class="mt-1.5 text-xs text-red-600 dark:text-red-400">
            {{ choiceError }}
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
              class="glass-inset mt-1.5 w-full rounded-lg px-3 py-2 text-sm dark:text-neutral-100"
            />
          </div>
          <div>
            <label class="block text-sm font-semibold text-neutral-700 dark:text-neutral-200">
              Domain
            </label>
            <input
              v-model="domain"
              type="text"
              class="glass-inset mt-1.5 w-full rounded-lg px-3 py-2 font-mono text-sm dark:text-neutral-100"
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
          class="glass-accent rounded-full px-4 py-2 text-sm font-semibold transition disabled:opacity-50"
          :disabled="
            !preview ||
            store.linking ||
            !name.trim() ||
            !domain.trim() ||
            choiceError !== null ||
            choicePending
          "
          @click="confirm"
        >
          {{ store.linking ? 'Adding…' : 'Add project' }}
        </button>
      </div>
    </div>
  </div>
</template>
