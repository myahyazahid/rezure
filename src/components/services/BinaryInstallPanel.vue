<script setup lang="ts">
import { computed } from 'vue'
import { useBinariesStore } from '@/stores/binaries'
import BasePill from '@/components/common/BasePill.vue'

const store = useBinariesStore()

const allInstalled = computed(
  () => store.binaries.length > 0 && store.binaries.every((b) => b.installed),
)

function stageLabel(id: string) {
  const stage = store.progressFor(id)?.stage
  switch (stage) {
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

function progressPercent(id: string) {
  const p = store.progressFor(id)
  if (!p || !p.totalBytes) return null
  return Math.min(100, Math.round((p.downloadedBytes / p.totalBytes) * 100))
}
</script>

<template>
  <section v-if="!allInstalled">
    <h2 class="text-sm font-semibold text-neutral-500">Portable binaries</h2>
    <p class="mt-0.5 text-xs text-neutral-500">
      Downloaded on demand from each project's official release — never bundled with the app.
    </p>

    <div class="mt-3 flex flex-col gap-2.5">
      <div
        v-for="binary in store.binaries"
        :key="binary.id"
        class="rounded-2xl border border-neutral-200/80 bg-neutral-100/60 p-3.5 dark:border-neutral-800 dark:bg-neutral-900/60"
      >
        <div class="flex items-center gap-3">
          <div
            class="flex h-9 w-9 shrink-0 items-center justify-center rounded-full text-sm font-semibold"
            :class="
              binary.installed
                ? 'bg-emerald-100 text-emerald-700 dark:bg-emerald-500/15 dark:text-emerald-400'
                : 'bg-neutral-200/70 text-neutral-500 dark:bg-neutral-800 dark:text-neutral-400'
            "
          >
            {{ binary.name.charAt(0).toUpperCase() }}
          </div>

          <div class="min-w-0 flex-1">
            <div class="flex items-center gap-2">
              <span class="truncate font-semibold text-neutral-900 dark:text-neutral-100">{{
                binary.name
              }}</span>
              <BasePill variant="mono">v{{ binary.version }}</BasePill>
            </div>
            <p class="mt-0.5 text-xs text-neutral-500">
              <template v-if="store.isInstalling(binary.id)">{{
                stageLabel(binary.id)
              }}</template>
              <template v-else>{{ binary.installed ? 'Installed' : 'Not installed' }}</template>
            </p>
          </div>

          <button
            v-if="!binary.installed"
            type="button"
            class="shrink-0 rounded-lg bg-red-600 px-3.5 py-1.5 text-sm font-semibold text-white shadow-sm shadow-red-600/30 transition hover:bg-red-500 disabled:opacity-50"
            :disabled="store.isInstalling(binary.id)"
            @click="store.install(binary.id)"
          >
            {{ store.isInstalling(binary.id) ? 'Installing…' : 'Install' }}
          </button>
        </div>

        <div
          v-if="store.isInstalling(binary.id)"
          class="mt-3 h-1.5 overflow-hidden rounded-full bg-neutral-200/70 dark:bg-neutral-800"
        >
          <div
            class="h-full rounded-full bg-red-500 transition-all"
            :class="progressPercent(binary.id) === null ? 'w-1/3 animate-pulse' : ''"
            :style="
              progressPercent(binary.id) !== null
                ? { width: `${progressPercent(binary.id)}%` }
                : undefined
            "
          ></div>
        </div>
      </div>
    </div>
  </section>
</template>
