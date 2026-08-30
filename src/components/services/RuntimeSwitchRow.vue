<script setup lang="ts">
import { ref } from 'vue'

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
    /** For runtimes with no bundled binary source yet (Node.js, Python). */
    disabled?: boolean
  }>(),
  { installingId: null, disabled: false },
)

const emit = defineEmits<{ select: [id: string]; install: [id: string] }>()

const open = ref(false)

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
    class="flex items-center gap-3 rounded-2xl border p-3.5 transition"
    :class="
      disabled
        ? 'border-neutral-200/60 bg-neutral-100/30 opacity-60 dark:border-neutral-800/60 dark:bg-neutral-900/30'
        : 'border-neutral-200/80 bg-neutral-100/60 dark:border-neutral-800 dark:bg-neutral-900/60'
    "
  >
    <span
      class="flex h-9 w-9 shrink-0 items-center justify-center rounded-full bg-red-50 text-sm font-bold text-red-600 dark:bg-red-500/10 dark:text-red-400"
    >
      {{ icon }}
    </span>

    <div class="min-w-0 flex-1">
      <p class="font-semibold text-neutral-900 dark:text-neutral-100">{{ name }}</p>
      <p class="mt-0.5 font-mono text-xs text-neutral-500">
        {{
          disabled
            ? 'not available yet'
            : activeVersion
              ? `active ${activeVersion}`
              : 'not installed'
        }}
      </p>
    </div>

    <span v-if="!disabled" class="shrink-0 text-xs text-neutral-500"
      >{{ installedCount }} installed</span
    >

    <div v-if="!disabled" class="relative shrink-0">
      <button
        type="button"
        class="flex items-center gap-1.5 rounded-full border border-neutral-200 bg-white/70 px-3 py-1.5 font-mono text-sm font-semibold text-neutral-700 transition hover:bg-white dark:border-neutral-700 dark:bg-neutral-900/60 dark:text-neutral-200 dark:hover:bg-neutral-800"
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

    <span
      v-if="!disabled && installedCount > 0"
      class="flex shrink-0 items-center gap-1 rounded-full bg-emerald-100 px-2.5 py-1 text-xs font-semibold text-emerald-700 dark:bg-emerald-500/15 dark:text-emerald-400"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" class="h-3 w-3">
        <path stroke-linecap="round" stroke-linejoin="round" d="m5 12 4 4L19 6" />
      </svg>
      In use
    </span>
  </div>
</template>
