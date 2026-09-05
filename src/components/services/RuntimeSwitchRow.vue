<script setup lang="ts">
import { computed, ref } from 'vue'
import BasePill from '@/components/common/BasePill.vue'
import type { InstallProgress } from '@/types/binary'

export interface RuntimeVersionEntry {
  id: string
  version: string
  installed: boolean
}

const props = withDefaults(
  defineProps<{
    icon: string
    name: string
    activeVersion: string | null
    installedCount: number
    versions: RuntimeVersionEntry[]
    installingId?: string | null
    /** Download/verify/extract progress for the install in flight. Null
     *  while a runtime is installing but reports no byte counts (Composer),
     *  which shows an indeterminate bar rather than a stuck 0%. */
    progress?: InstallProgress | null
    /** For runtimes with no bundled binary source yet (Node.js, Python). */
    disabled?: boolean
    /** A switch is in flight. Distinct from `disabled`: the row stays fully
     *  visible and keeps reporting its version — only picking another one is
     *  refused, so a second click can't race the switch already running. */
    busy?: boolean
  }>(),
  { installingId: null, progress: null, disabled: false, busy: false },
)

const emit = defineEmits<{ select: [id: string]; install: [id: string] }>()

const open = ref(false)

const installing = computed(() => props.installingId !== null)

/** Null while the download hasn't reported a total — a large binary sends
 *  its first bytes before the server's content length is known. */
const percent = computed(() => {
  const p = props.progress
  if (!p?.totalBytes) return null
  return Math.min(100, Math.round((p.downloadedBytes / p.totalBytes) * 100))
})

const stageLabel = computed(() => {
  switch (props.progress?.stage) {
    case 'downloading':
      return percent.value === null ? 'Downloading…' : `Downloading… ${percent.value}%`
    case 'verifying':
      return 'Verifying…'
    case 'extracting':
      return 'Extracting…'
    default:
      return 'Installing…'
  }
})

function pick(entry: RuntimeVersionEntry) {
  if (entry.installed) {
    emit('select', entry.id)
    open.value = false
  } else {
    emit('install', entry.id)
  }
}
</script>

<template>
  <div
    class="px-4 py-3.5 transition"
    :class="disabled ? 'opacity-50' : 'hover:bg-neutral-100/70 dark:hover:bg-neutral-800/40'"
  >
    <div class="flex items-center gap-3">
      <span
        class="flex h-9 w-9 shrink-0 items-center justify-center rounded-full text-sm font-bold"
        :class="
          !disabled && installedCount > 0
            ? 'bg-red-50 text-red-600 dark:bg-red-500/10 dark:text-red-400'
            : 'bg-neutral-200/70 text-neutral-500 dark:bg-neutral-800 dark:text-neutral-400'
        "
      >
        {{ icon }}
      </span>

      <div class="min-w-0 flex-1">
        <p class="font-semibold text-neutral-900 dark:text-neutral-100">{{ name }}</p>
        <div class="mt-0.5 flex items-center gap-1.5 text-xs">
          <span
            v-if="!disabled && !installing"
            class="h-1.5 w-1.5 shrink-0 rounded-full"
            :class="activeVersion ? 'bg-emerald-500' : 'bg-neutral-400 dark:bg-neutral-600'"
          ></span>
          <span
            class="font-mono"
            :class="
              installing
                ? 'text-red-600 dark:text-red-400'
                : !disabled && activeVersion
                  ? 'text-emerald-600 dark:text-emerald-400'
                  : 'text-neutral-500'
            "
          >
            {{
              installing
                ? stageLabel
                : disabled
                  ? 'not available yet'
                  : activeVersion
                    ? `active ${activeVersion}`
                    : 'not installed'
            }}
          </span>
        </div>
      </div>

      <BasePill v-if="!disabled" class="shrink-0">{{ installedCount }} installed</BasePill>

      <div v-if="!disabled" class="relative shrink-0">
        <button
          type="button"
          class="flex items-center gap-1.5 rounded-full border border-neutral-200 bg-white/70 px-3 py-1.5 font-mono text-sm font-semibold text-neutral-700 transition hover:bg-white disabled:cursor-wait disabled:opacity-50 dark:border-neutral-700 dark:bg-neutral-900/60 dark:text-neutral-200 dark:hover:bg-neutral-800"
          :disabled="busy"
          @click="open = !open"
        >
          {{ activeVersion ?? '—' }}
          <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            class="h-3.5 w-3.5 transition-transform"
            :class="open ? 'rotate-180' : ''"
          >
            <path stroke-linecap="round" stroke-linejoin="round" d="m6 9 6 6 6-6" />
          </svg>
        </button>

        <template v-if="open">
          <div class="fixed inset-0 z-10" @click="open = false"></div>
          <div
            class="absolute top-full right-0 z-20 mt-2 w-48 rounded-xl border border-neutral-200 bg-white p-1 shadow-lg dark:border-neutral-700 dark:bg-neutral-900"
          >
            <button
              v-for="(entry, i) in versions"
              :key="entry.id"
              type="button"
              class="flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left font-mono text-sm transition disabled:cursor-wait"
              :class="
                entry.installed
                  ? 'hover:bg-neutral-100 dark:hover:bg-neutral-800'
                  : 'text-neutral-400 hover:bg-neutral-100 dark:text-neutral-500 dark:hover:bg-neutral-800'
              "
              :disabled="installingId === entry.id"
              @click="pick(entry)"
            >
              <span
                class="h-1.5 w-1.5 shrink-0 rounded-full"
                :class="
                  entry.id === activeVersion || entry.version === activeVersion
                    ? 'bg-red-500'
                    : 'bg-transparent'
                "
              ></span>
              <span class="flex-1 truncate">{{ entry.version }}</span>
              <span v-if="i === 0" class="text-[10px] text-neutral-400">latest</span>
              <span v-if="installingId === entry.id" class="text-[10px] text-neutral-400"
                >installing…</span
              >
              <svg
                v-else-if="!entry.installed"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                class="h-3.5 w-3.5 shrink-0"
                aria-label="Download"
              >
                <path
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  d="M12 3v12m0 0 4-4m-4 4-4-4M5 19h14"
                />
              </svg>
            </button>
          </div>
        </template>
      </div>
    </div>

    <div
      v-if="installing"
      class="mt-3 h-1.5 overflow-hidden rounded-full bg-neutral-200/70 dark:bg-neutral-800"
    >
      <div
        class="h-full rounded-full bg-red-500 transition-all"
        :class="percent === null ? 'w-1/3 animate-pulse' : ''"
        :style="percent !== null ? { width: `${percent}%` } : undefined"
      ></div>
    </div>
  </div>
</template>
