<script setup lang="ts">
import { computed, ref } from 'vue'
import type { ServiceInfo } from '@/types/service'
import { useServicesStore } from '@/stores/services'
import BasePill from '@/components/common/BasePill.vue'
import ServiceLogPanel from '@/components/services/ServiceLogPanel.vue'

const props = defineProps<{ service: ServiceInfo }>()

const store = useServicesStore()
const expanded = ref(false)

const isRunning = computed(() => props.service.status === 'running')
const isPending = computed(() => store.isPending(props.service.id))
const initial = computed(() => props.service.name.charAt(0).toUpperCase())

function toggleExpanded() {
  expanded.value = !expanded.value
}

function onPrimaryAction() {
  return isRunning.value ? store.stop(props.service.id) : store.start(props.service.id)
}

function onRestart() {
  return store.restart(props.service.id)
}
</script>

<template>
  <div
    class="rounded-xl border border-neutral-200 bg-white dark:border-neutral-800 dark:bg-neutral-900"
  >
    <div class="flex items-center gap-3 p-4">
      <div
        class="flex h-9 w-9 shrink-0 items-center justify-center rounded-full text-sm font-semibold"
        :class="
          isRunning
            ? 'bg-emerald-100 text-emerald-700 dark:bg-emerald-500/15 dark:text-emerald-400'
            : 'bg-neutral-100 text-neutral-500 dark:bg-neutral-800 dark:text-neutral-400'
        "
      >
        {{ initial }}
      </div>

      <div class="min-w-0 flex-1">
        <div class="flex items-center gap-2">
          <span class="truncate font-semibold text-neutral-900 dark:text-neutral-100">{{
            service.name
          }}</span>
          <BasePill class="shrink-0">{{ service.category }}</BasePill>
        </div>
        <div class="mt-1 flex items-center gap-1.5 text-sm">
          <span
            class="h-1.5 w-1.5 rounded-full"
            :class="isRunning ? 'bg-emerald-500' : 'bg-neutral-400 dark:bg-neutral-600'"
          ></span>
          <span :class="isRunning ? 'text-emerald-600 dark:text-emerald-400' : 'text-neutral-500'">
            {{ isRunning ? 'Running' : 'Stopped' }}
          </span>
        </div>
      </div>

      <BasePill variant="mono" class="shrink-0">{{ service.version }}</BasePill>
      <BasePill variant="mono" class="shrink-0">:{{ service.port }}</BasePill>

      <button
        type="button"
        class="shrink-0 rounded-lg px-4 py-1.5 text-sm font-semibold transition disabled:opacity-50"
        :class="
          isRunning
            ? 'bg-neutral-100 text-neutral-700 hover:bg-neutral-200 dark:bg-neutral-800 dark:text-neutral-200 dark:hover:bg-neutral-700'
            : 'bg-red-600 text-white hover:bg-red-500'
        "
        :disabled="isPending"
        @click="onPrimaryAction"
      >
        {{ isRunning ? 'Stop' : 'Start' }}
      </button>

      <button
        type="button"
        title="Restart"
        class="flex h-8 w-8 shrink-0 items-center justify-center rounded-full border border-neutral-200 text-neutral-500 transition hover:bg-neutral-100 disabled:opacity-50 dark:border-neutral-700 dark:text-neutral-400 dark:hover:bg-neutral-800"
        :disabled="isPending"
        @click="onRestart"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="h-4 w-4">
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            d="M4.5 12a7.5 7.5 0 0 1 12.8-5.3L20 9M20 9V4M20 9h-5"
          />
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            d="M19.5 12a7.5 7.5 0 0 1-12.8 5.3L4 15m0 0v5m0-5h5"
          />
        </svg>
      </button>

      <button
        type="button"
        title="Toggle logs"
        class="flex h-8 w-8 shrink-0 items-center justify-center rounded-full border border-neutral-200 text-neutral-500 transition hover:bg-neutral-100 dark:border-neutral-700 dark:text-neutral-400 dark:hover:bg-neutral-800"
        @click="toggleExpanded"
      >
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          class="h-4 w-4 transition-transform"
          :class="expanded ? 'rotate-180' : ''"
        >
          <path stroke-linecap="round" stroke-linejoin="round" d="m19.5 8.25-7.5 7.5-7.5-7.5" />
        </svg>
      </button>
    </div>

    <ServiceLogPanel v-if="expanded" :service-id="service.id" />
  </div>
</template>
