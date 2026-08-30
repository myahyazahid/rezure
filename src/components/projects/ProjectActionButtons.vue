<script setup lang="ts">
import { useProjectsStore } from '@/stores/projects'

const props = defineProps<{ projectId: string; domain: string; path: string }>()

const store = useProjectsStore()

const ICON_BUTTON_CLASS =
  'flex h-9 w-9 shrink-0 items-center justify-center rounded-full border border-neutral-200 bg-white text-neutral-500 transition hover:border-neutral-300 hover:text-neutral-800 dark:border-neutral-700 dark:bg-neutral-800/60 dark:text-neutral-400 dark:hover:text-neutral-100'
</script>

<template>
  <div class="flex shrink-0 items-center gap-1.5">
    <button
      type="button"
      class="flex h-9 shrink-0 items-center gap-1.5 rounded-full bg-red-600 px-3.5 text-sm font-semibold text-white transition hover:bg-red-500"
      :title="`Open http://${props.domain}`"
      @click="store.openSite(props.projectId)"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="h-4 w-4">
        <circle cx="12" cy="12" r="9" />
        <path
          stroke-linecap="round"
          d="M3 12h18M12 3c2.5 2.7 2.5 15.3 0 18M12 3c-2.5 2.7-2.5 15.3 0 18"
        />
      </svg>
      Open
    </button>

    <button
      type="button"
      :class="ICON_BUTTON_CLASS"
      :title="`Open ${props.path} in Explorer`"
      @click="store.openFolder(props.projectId)"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="h-4 w-4">
        <path
          stroke-linecap="round"
          stroke-linejoin="round"
          d="M3 7a2 2 0 0 1 2-2h3.6l2 2.5H19a2 2 0 0 1 2 2V17a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z"
        />
      </svg>
    </button>

    <button
      type="button"
      :class="ICON_BUTTON_CLASS"
      title="Open a terminal in this folder"
      @click="store.openTerminal(props.projectId)"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="h-4 w-4">
        <rect x="3" y="4" width="18" height="16" rx="2" />
        <path stroke-linecap="round" stroke-linejoin="round" d="m7 9 3 3-3 3M13 15h4" />
      </svg>
    </button>
  </div>
</template>
