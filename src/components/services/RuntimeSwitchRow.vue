<script setup lang="ts">
import { ref } from 'vue'

defineProps<{
  icon: string
  name: string
  activeVersion: string
  installedCount: number
  versions: string[]
}>()

const emit = defineEmits<{ select: [version: string] }>()

const open = ref(false)

function select(version: string) {
  emit('select', version)
  open.value = false
}
</script>

<template>
  <div
    class="flex items-center gap-3 rounded-2xl border border-neutral-200/80 bg-neutral-100/60 p-3.5 dark:border-neutral-800 dark:bg-neutral-900/60"
  >
    <span
      class="flex h-9 w-9 shrink-0 items-center justify-center rounded-full bg-red-50 text-sm font-bold text-red-600 dark:bg-red-500/10 dark:text-red-400"
    >
      {{ icon }}
    </span>

    <div class="min-w-0 flex-1">
      <p class="font-semibold text-neutral-900 dark:text-neutral-100">{{ name }}</p>
      <p class="mt-0.5 font-mono text-xs text-neutral-500">active {{ activeVersion }}</p>
    </div>

    <span class="shrink-0 text-xs text-neutral-500">{{ installedCount }} installed</span>

    <div class="relative shrink-0">
      <button
        type="button"
        class="flex items-center gap-1.5 rounded-full border border-neutral-200 bg-white/70 px-3 py-1.5 font-mono text-sm font-semibold text-neutral-700 transition hover:bg-white dark:border-neutral-700 dark:bg-neutral-900/60 dark:text-neutral-200 dark:hover:bg-neutral-800"
        @click="open = !open"
      >
        {{ activeVersion }}
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
          class="absolute top-full right-0 z-20 mt-2 w-36 rounded-xl border border-neutral-200 bg-white p-1 shadow-lg dark:border-neutral-700 dark:bg-neutral-900"
        >
          <button
            v-for="(version, i) in versions"
            :key="version"
            type="button"
            class="flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left font-mono text-sm transition hover:bg-neutral-100 dark:hover:bg-neutral-800"
            @click="select(version)"
          >
            <span
              class="h-1.5 w-1.5 shrink-0 rounded-full"
              :class="version === activeVersion ? 'bg-red-500' : 'bg-transparent'"
            ></span>
            <span class="flex-1 truncate">{{ version }}</span>
            <span v-if="i === 0" class="text-[10px] text-neutral-400">latest</span>
          </button>
        </div>
      </template>
    </div>

    <span
      class="shrink-0 rounded-full bg-white/80 px-2.5 py-1 text-xs font-semibold text-neutral-500 dark:bg-neutral-900/60 dark:text-neutral-400"
    >
      Active
    </span>
  </div>
</template>
