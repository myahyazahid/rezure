<script setup lang="ts">
import { computed, onMounted, onUnmounted } from 'vue'
import { open as openFolderDialog } from '@tauri-apps/plugin-dialog'
import { usePhpStore } from '@/stores/php'

const emit = defineEmits<{ close: [] }>()

const store = usePhpStore()

const busy = computed(() => store.installingId !== null || store.adding)

function close() {
  if (busy.value) return
  emit('close')
}

function stageLabel(version: string) {
  switch (store.progressFor(version)?.stage) {
    case 'downloading':
      return 'Downloading…'
    case 'verifying':
      return 'Verifying…'
    case 'extracting':
      return 'Extracting…'
    default:
      return 'Installing…'
  }
}

function progressPercent(version: string) {
  const p = store.progressFor(version)
  if (!p || !p.totalBytes) return null
  return Math.min(100, Math.round((p.downloadedBytes / p.totalBytes) * 100))
}

async function pickFolder() {
  const picked = await openFolderDialog({ directory: true, multiple: false })
  if (typeof picked === 'string') await store.addFromFolder(picked)
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') close()
}

onMounted(() => {
  window.addEventListener('keydown', onKeydown)
  if (store.catalog.length === 0) store.fetchCatalog()
  if (!store.dropInDir) store.fetchDropInDir()
})
onUnmounted(() => window.removeEventListener('keydown', onKeydown))
</script>

<template>
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4 backdrop-blur-sm"
    @click.self="close"
  >
    <div
      class="flex max-h-[85vh] w-full max-w-xl flex-col rounded-3xl bg-white shadow-2xl dark:bg-neutral-900"
    >
      <div class="flex items-start justify-between gap-4 p-6 pb-4">
        <div class="min-w-0">
          <h2 class="text-xl font-bold tracking-tight">Install a PHP version</h2>
          <p class="mt-0.5 text-sm text-neutral-500">
            Rezure downloads the build from php.net and verifies its checksum — nothing else on your
            machine changes.
          </p>
        </div>
        <button
          type="button"
          class="flex h-8 w-8 shrink-0 items-center justify-center rounded-full border border-neutral-200 text-neutral-500 transition hover:bg-neutral-100 disabled:opacity-40 dark:border-neutral-700 dark:hover:bg-neutral-800"
          :disabled="busy"
          title="Close"
          @click="close"
        >
          <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            class="h-4 w-4"
          >
            <path stroke-linecap="round" d="M6 6l12 12M18 6L6 18" />
          </svg>
        </button>
      </div>

      <div class="min-h-0 flex-1 overflow-y-auto px-6">
        <p v-if="store.catalogLoading" class="py-6 text-center text-sm text-neutral-500">
          Reading php.net's release index…
        </p>

        <div
          v-else-if="store.catalog.length === 0"
          class="rounded-2xl border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-900 dark:border-amber-500/25 dark:bg-amber-500/10 dark:text-amber-200"
        >
          Couldn't reach php.net's release index, so there's nothing to install from here. You can
          still add a build you've downloaded yourself, below.
          <button
            type="button"
            class="ml-1 font-semibold underline"
            @click="store.fetchCatalog(true)"
          >
            Retry
          </button>
        </div>

        <div v-else class="flex flex-col gap-2">
          <div
            v-for="release in store.catalog"
            :key="release.version"
            class="rounded-2xl border border-neutral-200 bg-neutral-50/70 p-3.5 dark:border-neutral-800 dark:bg-neutral-900/60"
          >
            <div class="flex items-center gap-3">
              <div class="min-w-0 flex-1">
                <div class="flex items-center gap-2">
                  <span class="font-mono font-semibold text-neutral-900 dark:text-neutral-100">
                    {{ release.version }}
                  </span>
                  <span
                    v-if="release.latest"
                    class="rounded-full bg-red-100 px-2 py-0.5 text-[10px] font-bold tracking-wide text-red-600 uppercase dark:bg-red-500/15 dark:text-red-400"
                  >
                    Latest
                  </span>
                  <span
                    v-else
                    class="rounded-full bg-neutral-200/70 px-2 py-0.5 text-[10px] font-bold tracking-wide text-neutral-500 uppercase dark:bg-neutral-800 dark:text-neutral-400"
                  >
                    {{ release.branch }}
                  </span>
                </div>
                <p class="mt-0.5 text-xs text-neutral-500">
                  <template v-if="store.installingId === release.version">
                    {{ stageLabel(release.version) }}
                  </template>
                  <template v-else> Released {{ release.released }} · {{ release.size }} </template>
                </p>
              </div>

              <button
                v-if="!release.installed"
                type="button"
                class="shrink-0 rounded-full bg-red-600 px-4 py-2 text-sm font-semibold text-white shadow-sm shadow-red-600/30 transition hover:bg-red-500 disabled:opacity-50"
                :disabled="busy"
                @click="store.install(release.version)"
              >
                {{ store.installingId === release.version ? 'Installing…' : 'Install' }}
              </button>
              <span
                v-else
                class="shrink-0 rounded-full border border-neutral-200 px-4 py-2 text-sm font-semibold text-neutral-400 dark:border-neutral-700 dark:text-neutral-500"
              >
                Installed
              </span>
            </div>

            <div
              v-if="store.installingId === release.version"
              class="mt-3 h-1.5 overflow-hidden rounded-full bg-neutral-200/70 dark:bg-neutral-800"
            >
              <div
                class="h-full rounded-full bg-red-500 transition-all"
                :class="progressPercent(release.version) === null ? 'w-1/3 animate-pulse' : ''"
                :style="
                  progressPercent(release.version) !== null
                    ? { width: `${progressPercent(release.version)}%` }
                    : undefined
                "
              ></div>
            </div>
          </div>
        </div>

        <!-- The second way in: a build the user downloaded themselves. Same
             list, same switching — the only difference is that Rezure never
             checksum-verified it, which the Switch row marks. -->
        <div class="mt-5 border-t border-neutral-200 pt-4 dark:border-neutral-800">
          <h3 class="text-sm font-semibold text-neutral-900 dark:text-neutral-100">
            Already downloaded one?
          </h3>
          <p class="mt-0.5 text-xs text-neutral-500">
            php.net only publishes the newest release of each branch. For anything older — or a
            build you already have — point Rezure at the folder, or unpack it into the folder below
            and it'll be picked up on the next refresh.
          </p>

          <div class="mt-3 flex flex-wrap items-center gap-2">
            <button
              type="button"
              class="flex items-center gap-2 rounded-full bg-neutral-900 px-4 py-2 text-sm font-semibold text-white transition hover:bg-neutral-800 disabled:opacity-50 dark:bg-neutral-100 dark:text-neutral-900 dark:hover:bg-white"
              :disabled="busy"
              @click="pickFolder"
            >
              <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                class="h-4 w-4"
              >
                <path
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  d="M3 7a2 2 0 0 1 2-2h3.6l2 2.5H19a2 2 0 0 1 2 2V17a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z"
                />
              </svg>
              {{ store.adding ? 'Copying…' : 'Add from folder…' }}
            </button>
            <button
              type="button"
              class="rounded-full border border-neutral-200 px-4 py-2 text-sm font-semibold text-neutral-700 transition hover:bg-neutral-100 dark:border-neutral-700 dark:text-neutral-200 dark:hover:bg-neutral-800"
              @click="store.openDropInDir"
            >
              Open folder
            </button>
          </div>

          <p
            v-if="store.dropInDir"
            class="mt-2 truncate font-mono text-xs text-neutral-400"
            :title="store.dropInDir"
          >
            {{ store.dropInDir }}
          </p>
        </div>

        <p v-if="store.catalogError" class="mt-3 text-sm text-red-600 dark:text-red-400">
          {{ store.catalogError }}
        </p>
      </div>

      <div
        class="flex items-center justify-between gap-3 border-t border-neutral-200 p-6 pt-4 dark:border-neutral-800"
      >
        <span class="text-xs text-neutral-400">
          Installed versions show up in the Switch dropdown right away.
        </span>
        <button
          type="button"
          class="shrink-0 rounded-full border border-neutral-200 px-5 py-2.5 text-sm font-semibold text-neutral-700 transition hover:bg-neutral-100 disabled:opacity-50 dark:border-neutral-700 dark:text-neutral-200 dark:hover:bg-neutral-800"
          :disabled="busy"
          @click="close"
        >
          Done
        </button>
      </div>
    </div>
  </div>
</template>
