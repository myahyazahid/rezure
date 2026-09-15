<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from 'vue'
import { useProjectsStore } from '@/stores/projects'
import LeafLoader from '@/components/common/LeafLoader.vue'

const store = useProjectsStore()

const project = computed(() => store.projects.find((p) => p.id === store.shareModalFor) ?? null)
const url = computed(() =>
  store.shareModalFor ? (store.shareUrls[store.shareModalFor] ?? null) : null,
)
const isLoading = computed(
  () => store.shareModalFor !== null && store.sharingFor === store.shareModalFor,
)

const copied = ref(false)

async function copyUrl() {
  if (!url.value) return
  await navigator.clipboard.writeText(url.value)
  copied.value = true
  setTimeout(() => (copied.value = false), 1500)
}

function stop() {
  if (store.shareModalFor) store.stopSharing(store.shareModalFor)
}

/** Cycled while waiting on `share_project` — a single static "Loading…"
 *  reads as stuck once cloudflared's first-run download takes more than a
 *  couple of seconds, so this narrates roughly where the wait is going. */
const LOADING_MESSAGES = ['Setting up your project…', 'Starting the tunnel…', 'Almost done…']
const loadingMessageIndex = ref(0)
let loadingTimer: ReturnType<typeof setInterval> | null = null

watch(
  isLoading,
  (loading) => {
    if (loadingTimer) {
      clearInterval(loadingTimer)
      loadingTimer = null
    }
    if (loading) {
      loadingMessageIndex.value = 0
      loadingTimer = setInterval(() => {
        loadingMessageIndex.value = (loadingMessageIndex.value + 1) % LOADING_MESSAGES.length
      }, 1800)
    }
  },
  { immediate: true },
)

onUnmounted(() => {
  if (loadingTimer) clearInterval(loadingTimer)
})
</script>

<template>
  <div
    v-if="store.shareModalFor"
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
    @click.self="store.closeShareModal()"
  >
    <div
      class="w-full max-w-lg rounded-2xl border border-neutral-200 bg-white p-6 shadow-2xl dark:border-neutral-700 dark:bg-neutral-900"
    >
      <h2 class="text-lg font-bold text-neutral-900 dark:text-neutral-100">
        Sharing this project
        <span v-if="project" class="font-normal text-neutral-400">· {{ project.name }}</span>
      </h2>
      <p class="mt-1 text-sm text-neutral-500">
        Anyone with this link can reach it while sharing stays on — nobody needs to be on your
        network.
      </p>

      <div v-if="isLoading" class="mt-6 flex flex-col items-center gap-2 py-4 text-center">
        <LeafLoader :size="48" />
        <p class="mt-1 text-sm font-semibold text-neutral-700 dark:text-neutral-200">
          {{ LOADING_MESSAGES[loadingMessageIndex] }}
        </p>
        <p class="text-xs text-neutral-400">
          First-time setup also downloads the tunnel binary — this can take a few seconds.
        </p>
      </div>

      <template v-else-if="url">
        <div
          class="mt-5 flex items-center gap-2 rounded-xl border border-neutral-200 bg-neutral-50 px-3 py-2.5 dark:border-neutral-700 dark:bg-neutral-800/60"
        >
          <code class="flex-1 truncate font-mono text-sm text-neutral-800 dark:text-neutral-200">{{
            url
          }}</code>
          <button
            type="button"
            class="shrink-0 rounded-full border border-neutral-200 px-3 py-1 text-xs font-semibold text-neutral-700 transition hover:bg-neutral-100 dark:border-neutral-700 dark:text-neutral-200 dark:hover:bg-neutral-800"
            @click="copyUrl"
          >
            {{ copied ? 'Copied!' : 'Copy' }}
          </button>
        </div>

        <!-- Cloudflare's own quick tunnels can take a short moment to
             propagate before they answer from outside your network — a
             fresh link failing to load right away doesn't mean it's broken. -->
        <p class="mt-3 text-xs text-neutral-400">
          New links can take up to a minute to become reachable from another device. If it doesn't
          load right away, wait a bit and try again.
        </p>
      </template>

      <p
        v-if="store.shareError"
        class="mt-4 rounded-xl bg-amber-50 px-3 py-2 text-sm text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
      >
        {{ store.shareError }}
      </p>

      <div class="mt-6 flex items-center justify-end gap-2">
        <button
          v-if="url"
          type="button"
          class="rounded-full border border-neutral-200 px-4 py-2 text-sm font-semibold text-neutral-700 transition hover:bg-neutral-100 dark:border-neutral-700 dark:text-neutral-200 dark:hover:bg-neutral-800"
          @click="stop"
        >
          Stop sharing
        </button>
        <button
          type="button"
          class="rounded-full bg-red-600 px-4 py-2 text-sm font-semibold text-white transition hover:bg-red-500"
          @click="store.closeShareModal()"
        >
          Close
        </button>
      </div>
    </div>
  </div>
</template>
