<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { open as openFolderDialog } from '@tauri-apps/plugin-dialog'
import { usePhpStore } from '@/stores/php'
import { useBinariesStore } from '@/stores/binaries'
import { useComposerStore } from '@/stores/composer'
import { useNodeStore } from '@/stores/node'
import TechIcon from '@/components/common/TechIcon.vue'
import CatalogVersionList from '@/components/services/CatalogVersionList.vue'

const emit = defineEmits<{ close: [] }>()

const phpStore = usePhpStore()
const binariesStore = useBinariesStore()
const composerStore = useComposerStore()
const nodeStore = useNodeStore()

type Runtime = 'php' | 'nginx' | 'mariadb' | 'composer' | 'node'

const RUNTIMES: { id: Runtime; label: string; icon: string }[] = [
  { id: 'php', label: 'PHP', icon: 'php' },
  { id: 'nginx', label: 'Nginx', icon: 'nginx' },
  { id: 'mariadb', label: 'MariaDB', icon: 'mariadb' },
  { id: 'composer', label: 'Composer', icon: 'composer' },
  { id: 'node', label: 'Node.js', icon: 'node' },
]

const selected = ref<Runtime | null>(null)

const nginxPackage = computed(() => binariesStore.binaries.find((b) => b.id === 'nginx') ?? null)

/** Any install in flight, across every runtime — closing mid-download would
 *  leave the frontend with no way to see it finish. */
const busy = computed(
  () =>
    phpStore.installingId !== null ||
    phpStore.adding ||
    binariesStore.isInstalling('nginx') ||
    binariesStore.installingMariaDbVersion !== null ||
    composerStore.installingVersion !== null ||
    nodeStore.installingVersion !== null,
)

function close() {
  if (busy.value) return
  emit('close')
}

function back() {
  if (busy.value) return
  selected.value = null
}

watch(selected, (runtime) => {
  if (runtime === 'php' && phpStore.catalog.length === 0) phpStore.fetchCatalog()
  if (runtime === 'nginx' && binariesStore.binaries.length === 0) binariesStore.fetchAll()
  if (runtime === 'mariadb' && binariesStore.mariadbCatalog.length === 0) {
    binariesStore.fetchMariaDbCatalog()
  }
  if (runtime === 'composer' && composerStore.catalog.length === 0) composerStore.fetchCatalog()
  if (runtime === 'node' && nodeStore.catalog.length === 0) nodeStore.fetchCatalog()
})

async function pickFolder() {
  const picked = await openFolderDialog({ directory: true, multiple: false })
  if (typeof picked === 'string') await phpStore.addFromFolder(picked)
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') close()
}

onMounted(() => {
  window.addEventListener('keydown', onKeydown)
  if (!phpStore.dropInDir) phpStore.fetchDropInDir()
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
          <div class="flex items-center gap-2">
            <button
              v-if="selected"
              type="button"
              class="flex h-7 w-7 shrink-0 items-center justify-center rounded-full text-neutral-500 transition hover:bg-neutral-100 disabled:opacity-40 dark:hover:bg-neutral-800"
              :disabled="busy"
              title="Choose a different runtime"
              @click="back"
            >
              <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2.5"
                class="h-4 w-4"
              >
                <path stroke-linecap="round" stroke-linejoin="round" d="m15 18-6-6 6-6" />
              </svg>
            </button>
            <h2 class="text-xl font-bold tracking-tight">
              {{
                selected
                  ? `Install ${RUNTIMES.find((r) => r.id === selected)?.label}`
                  : 'Install a version'
              }}
            </h2>
          </div>
          <p class="mt-0.5 text-sm text-neutral-500">
            {{
              selected
                ? 'Rezure downloads the build and verifies its checksum — nothing else on your machine changes.'
                : 'Pick a runtime to see what can be installed.'
            }}
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
        <!-- Step 1: pick a runtime -->
        <div v-if="!selected" class="grid grid-cols-2 gap-2 pb-2 sm:grid-cols-3">
          <button
            v-for="runtime in RUNTIMES"
            :key="runtime.id"
            type="button"
            class="flex flex-col items-center gap-2 rounded-2xl border border-neutral-200 bg-neutral-50/70 p-4 text-center transition hover:border-red-300 hover:bg-red-50/50 dark:border-neutral-800 dark:bg-neutral-900/60 dark:hover:border-red-500/40 dark:hover:bg-red-500/10"
            @click="selected = runtime.id"
          >
            <span
              class="flex h-10 w-10 items-center justify-center rounded-full bg-white dark:bg-neutral-800"
            >
              <TechIcon :id="runtime.icon" :size="22" />
            </span>
            <span class="text-sm font-semibold text-neutral-800 dark:text-neutral-200">
              {{ runtime.label }}
            </span>
          </button>
        </div>

        <!-- Step 2a: PHP — has its own catalog and drop-in folder -->
        <template v-else-if="selected === 'php'">
          <CatalogVersionList
            :releases="phpStore.catalog"
            :loading="phpStore.catalogLoading"
            :error="phpStore.catalogError"
            :installing-version="phpStore.installingId"
            :progress="phpStore.progressFor"
            source-label="php.net's release index"
            @install="phpStore.install"
            @retry="phpStore.fetchCatalog(true)"
          />

          <div class="mt-5 border-t border-neutral-200 pt-4 dark:border-neutral-800">
            <h3 class="text-sm font-semibold text-neutral-900 dark:text-neutral-100">
              Already downloaded one?
            </h3>
            <p class="mt-0.5 text-xs text-neutral-500">
              php.net only publishes the newest release of each branch. For anything older — or a
              build you already have — point Rezure at the folder, or unpack it into the folder
              below and it'll be picked up on the next refresh.
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
                {{ phpStore.adding ? 'Copying…' : 'Add from folder…' }}
              </button>
              <button
                type="button"
                class="rounded-full border border-neutral-200 px-4 py-2 text-sm font-semibold text-neutral-700 transition hover:bg-neutral-100 dark:border-neutral-700 dark:text-neutral-200 dark:hover:bg-neutral-800"
                @click="phpStore.openDropInDir"
              >
                Open folder
              </button>
            </div>

            <p
              v-if="phpStore.dropInDir"
              class="mt-2 truncate font-mono text-xs text-neutral-400"
              :title="phpStore.dropInDir"
            >
              {{ phpStore.dropInDir }}
            </p>
          </div>
        </template>

        <!-- Step 2b: Nginx — one pinned version, no live catalog (see the
             open question in docs/v3/rezure-app-v3-phases-tasks.md) -->
        <template v-else-if="selected === 'nginx'">
          <p v-if="binariesStore.loading" class="py-6 text-center text-sm text-neutral-500">
            Checking what's installed…
          </p>
          <div
            v-else-if="nginxPackage"
            class="rounded-2xl border border-neutral-200 bg-neutral-50/70 p-3.5 dark:border-neutral-800 dark:bg-neutral-900/60"
          >
            <div class="flex items-center gap-3">
              <div class="min-w-0 flex-1">
                <span class="font-mono font-semibold text-neutral-900 dark:text-neutral-100">
                  {{ nginxPackage.version }}
                </span>
                <p class="mt-0.5 text-xs text-neutral-500">
                  Only one build is offered for now — there's nothing to pick between yet.
                </p>
              </div>
              <button
                v-if="!nginxPackage.installed"
                type="button"
                class="shrink-0 rounded-full bg-red-600 px-4 py-2 text-sm font-semibold text-white shadow-sm shadow-red-600/30 transition hover:bg-red-500 disabled:opacity-50"
                :disabled="busy"
                @click="binariesStore.install('nginx')"
              >
                {{ binariesStore.isInstalling('nginx') ? 'Installing…' : 'Install' }}
              </button>
              <span
                v-else
                class="shrink-0 rounded-full border border-neutral-200 px-4 py-2 text-sm font-semibold text-neutral-400 dark:border-neutral-700 dark:text-neutral-500"
              >
                Installed
              </span>
            </div>
          </div>
        </template>

        <!-- Step 2c: MariaDB — live catalog across a fixed branch list -->
        <template v-else-if="selected === 'mariadb'">
          <CatalogVersionList
            :releases="binariesStore.mariadbCatalog"
            :loading="binariesStore.mariadbCatalogLoading"
            :error="binariesStore.mariadbCatalogError"
            :installing-version="binariesStore.installingMariaDbVersion"
            :progress="(v) => binariesStore.progressFor(`mariadb-${v}`)"
            source-label="MariaDB's release index"
            @install="binariesStore.installMariaDbVersion"
            @retry="binariesStore.fetchMariaDbCatalog(true)"
          />
        </template>

        <!-- Step 2d: Composer -->
        <template v-else-if="selected === 'composer'">
          <CatalogVersionList
            :releases="composerStore.catalog"
            :loading="composerStore.catalogLoading"
            :error="composerStore.catalogError"
            :installing-version="composerStore.installingVersion"
            :progress="composerStore.progressFor"
            source-label="Composer's version index"
            @install="composerStore.installVersion"
            @retry="composerStore.fetchCatalog(true)"
          />
        </template>

        <!-- Step 2e: Node.js -->
        <template v-else-if="selected === 'node'">
          <CatalogVersionList
            :releases="nodeStore.catalog"
            :loading="nodeStore.catalogLoading"
            :error="nodeStore.catalogError"
            :installing-version="nodeStore.installingVersion"
            :progress="nodeStore.progressFor"
            source-label="nodejs.org's release index"
            @install="nodeStore.installVersion"
            @retry="nodeStore.fetchCatalog(true)"
          />
          <p class="mt-4 text-xs text-neutral-500">
            Installing puts Node.js on disk — using it from a project (PATH, per-project switching)
            is a separate feature that isn't built yet.
          </p>
        </template>
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
