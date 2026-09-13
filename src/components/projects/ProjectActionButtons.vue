<script setup lang="ts">
import { computed } from 'vue'
import { useProjectsStore } from '@/stores/projects'

const props = defineProps<{ projectId: string; domain: string; path: string }>()

const store = useProjectsStore()

const ICON_BUTTON_CLASS =
  'flex h-9 w-9 shrink-0 items-center justify-center rounded-full border border-neutral-200 bg-white text-neutral-500 transition hover:border-neutral-300 hover:text-neutral-800 dark:border-neutral-700 dark:bg-neutral-800/60 dark:text-neutral-400 dark:hover:text-neutral-100'

// A fixed-size icon button in every state (idle / starting / active) rather
// than growing into a URL chip when active: this sits in a list row whose
// Actions column has a fixed width (see ProjectsView.vue), and a chip wide
// enough for a `trycloudflare.com` URL plus copy/stop buttons overflowed
// past it into the Domain/Stack columns. The URL itself lives in
// ProjectShareModal, opened from here instead.
const isActive = computed(() => Boolean(store.shareUrls[props.projectId]))
const isStarting = computed(() => store.sharingFor === props.projectId)

function onShareClick() {
  if (isStarting.value) return
  if (isActive.value) {
    store.openShareModal(props.projectId)
  } else {
    store.shareProject(props.projectId)
  }
}
</script>

<template>
  <div class="flex shrink-0 flex-wrap items-center justify-end gap-1.5">
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

    <!-- Same shape and weight as the other icon buttons — a red-tinted
         outline rather than Open's solid fill, so it reads as "on-brand
         but secondary" and stays distinct from both Open and the neutral
         icons next to it. Emerald once a share is actually live. -->
    <button
      type="button"
      class="flex h-9 w-9 shrink-0 items-center justify-center rounded-full border bg-white transition disabled:cursor-wait dark:bg-neutral-800/60"
      :class="[
        isActive
          ? 'border-emerald-300 text-emerald-600 hover:bg-emerald-50 dark:border-emerald-700 dark:text-emerald-400 dark:hover:bg-emerald-500/10'
          : 'border-red-300 text-red-600 hover:bg-red-50 disabled:opacity-70 dark:border-red-800 dark:text-red-400 dark:hover:bg-red-500/10',
      ]"
      :disabled="isStarting"
      :title="
        isStarting
          ? 'Starting share…'
          : isActive
            ? 'This project is shared publicly — click for the link'
            : 'Share this project publicly (Cloudflare Quick Tunnel)'
      "
      @click="onShareClick"
    >
      <span
        v-if="isStarting"
        class="h-4 w-4 animate-spin rounded-full border-2 border-current border-t-transparent"
      />
      <svg
        v-else
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        class="h-4 w-4"
      >
        <path
          stroke-linecap="round"
          stroke-linejoin="round"
          d="M12 3v12M12 3l-4 4M12 3l4 4M5 15v3a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-3"
        />
      </svg>
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

    <!-- Runs `php -m` on demand, never on render: one process per project
         per page visit would be the wrong trade for a question nobody asked
         yet. -->
    <button
      type="button"
      :class="ICON_BUTTON_CLASS"
      title="Check this project's PHP extension requirements"
      @click="store.runDoctor(props.projectId)"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="h-4 w-4">
        <path
          stroke-linecap="round"
          stroke-linejoin="round"
          d="M6 3v6a4 4 0 0 0 8 0V3M10 17a4 4 0 0 0 8 0v-2"
        />
        <circle cx="18" cy="13" r="2" />
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
