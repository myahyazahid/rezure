<script setup lang="ts">
import { onMounted, onUnmounted, watch } from 'vue'
import { useProjectsStore } from '@/stores/projects'
import { useTypedConfirm } from '@/composables/useTypedConfirm'
import type { ProjectInfo } from '@/types/project'

const props = defineProps<{ project: ProjectInfo }>()
const emit = defineEmits<{ close: [] }>()

const store = useProjectsStore()
const { typed, confirmed } = useTypedConfirm()

function close() {
  if (store.deleting) return
  store.deleteError = null
  emit('close')
}

async function submit() {
  if (!confirmed.value || store.deleting) return
  const ok = await store.deleteProject(props.project.id)
  if (ok) close()
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') close()
}

onMounted(() => window.addEventListener('keydown', onKeydown))
onUnmounted(() => window.removeEventListener('keydown', onKeydown))

watch(typed, () => {
  store.deleteError = null
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
      aria-labelledby="delete-project-title"
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
          <h2 id="delete-project-title" class="text-xl font-bold tracking-tight">Delete project</h2>
          <p class="mt-0.5 truncate text-sm font-semibold text-neutral-700 dark:text-neutral-200">
            {{ project.name }}
          </p>
        </div>
      </div>

      <p class="mt-4 text-sm text-neutral-600 dark:text-neutral-300">
        The folder and everything in it goes to the
        <strong class="text-neutral-800 dark:text-neutral-100">Recycle Bin</strong>, and
        <span class="font-mono">{{ project.domain }}</span> stops being served. You can restore the
        folder from the Recycle Bin until you empty it.
      </p>
      <p class="mt-2 truncate font-mono text-xs text-neutral-500" :title="project.path">
        {{ project.path }}
      </p>
      <p class="mt-3 text-xs text-neutral-500">
        Its database, if it has one, is not touched - delete that from Databases if you want it gone
        too.
      </p>

      <label for="delete-project-confirm" class="mt-5 block text-xs font-medium text-neutral-500">
        Type <span class="font-mono font-semibold text-red-600 dark:text-red-400">CONFIRM</span>
        to delete
      </label>
      <input
        id="delete-project-confirm"
        v-model="typed"
        type="text"
        placeholder="CONFIRM"
        autocomplete="off"
        spellcheck="false"
        autofocus
        class="glass-inset mt-1 w-full rounded-xl px-3.5 py-2.5 font-mono text-sm text-neutral-900 outline-none focus:border-red-400/70 dark:text-neutral-100"
        @keyup.enter="submit"
      />

      <p v-if="store.deleteError" class="mt-3 text-sm text-red-600 dark:text-red-400">
        {{ store.deleteError }}
      </p>

      <div class="mt-5 flex justify-end gap-2">
        <button
          type="button"
          class="glass-btn rounded-full px-4 py-2 text-sm font-semibold text-neutral-700 transition disabled:opacity-50 dark:text-neutral-200"
          :disabled="store.deleting"
          @click="close"
        >
          Cancel
        </button>
        <button
          type="button"
          class="glass-accent rounded-full px-4 py-2 text-sm font-semibold text-red-600 transition disabled:cursor-not-allowed disabled:opacity-50 dark:text-red-400"
          :disabled="!confirmed || store.deleting"
          @click="submit"
        >
          {{ store.deleting ? 'Deleting…' : 'Delete project' }}
        </button>
      </div>
    </div>
  </div>
</template>
