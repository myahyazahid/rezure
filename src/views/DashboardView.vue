<script setup lang="ts">
import { computed, ref } from 'vue'
import { useServicesStore } from '@/stores/services'
import ServiceRow from '@/components/services/ServiceRow.vue'
import BusyOverlay from '@/components/common/BusyOverlay.vue'
import LeafLoader from '@/components/common/LeafLoader.vue'

const store = useServicesStore()

type BulkAction = 'start' | 'restart' | 'stop'

/**
 * Which bulk action is in flight, if any.
 *
 * Deliberately not `store.busy`: that is also true while a single row is
 * starting, and covering the whole window for one toggle would hide the row
 * state the user is already watching. Only the bulk buttons — which touch
 * every service at once and take seconds — earn the overlay.
 */
const bulk = ref<BulkAction | null>(null)

const BUSY_LABELS: Record<BulkAction, string> = {
  start: 'Starting services…',
  restart: 'Restarting services…',
  stop: 'Stopping services…',
}

const busyLabel = computed(() => (bulk.value ? BUSY_LABELS[bulk.value] : ''))

const BULK_ACTIONS: Record<BulkAction, () => Promise<unknown>> = {
  start: () => store.startAll(),
  restart: () => store.restartAll(),
  stop: () => store.stopAll(),
}

async function runBulk(kind: BulkAction) {
  if (bulk.value) return
  bulk.value = kind
  try {
    await BULK_ACTIONS[kind]()
  } finally {
    bulk.value = null
  }
}

const SECONDARY_BUTTON_CLASS =
  'flex items-center gap-2 rounded-full border border-neutral-200 bg-white/70 px-5 py-2.5 text-sm font-semibold text-neutral-700 transition hover:bg-white disabled:opacity-50 dark:border-neutral-700 dark:bg-neutral-900/60 dark:text-neutral-200 dark:hover:bg-neutral-800'
</script>

<template>
  <section>
    <div class="flex items-start justify-between gap-4">
      <div>
        <h1 class="text-[28px] leading-tight font-bold tracking-tight">Services</h1>
        <p class="mt-1 text-sm text-neutral-500">Your local stack, one tap away.</p>
      </div>

      <div class="flex shrink-0 items-center gap-2">
        <button
          type="button"
          class="flex items-center gap-2 rounded-full bg-red-600 px-5 py-2.5 text-sm font-semibold text-white shadow-lg shadow-red-500/40 transition hover:bg-red-500 disabled:opacity-50"
          :disabled="bulk !== null"
          @click="runBulk('start')"
        >
          <svg viewBox="0 0 10 10" fill="currentColor" aria-hidden="true" class="h-2.5 w-2.5">
            <path d="M1.5 0.8 9 5 1.5 9.2Z" />
          </svg>
          Start all
        </button>
        <button
          type="button"
          :class="SECONDARY_BUTTON_CLASS"
          :disabled="bulk !== null"
          title="Restart every running service"
          @click="runBulk('restart')"
        >
          <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2.5"
            aria-hidden="true"
            class="h-3.5 w-3.5"
            :class="bulk === 'restart' ? 'animate-spin' : ''"
          >
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
          Restart all
        </button>
        <button
          type="button"
          :class="SECONDARY_BUTTON_CLASS"
          :disabled="bulk !== null"
          @click="runBulk('stop')"
        >
          <svg viewBox="0 0 10 10" fill="currentColor" aria-hidden="true" class="h-2 w-2">
            <rect width="10" height="10" rx="1.5" />
          </svg>
          Stop all
        </button>
      </div>
    </div>

    <!-- First load only — a refetch keeps the existing rows on screen. -->
    <div v-if="store.loading && store.services.length === 0" class="mt-12 flex justify-center">
      <LeafLoader :size="52" label="Loading services…" />
    </div>
    <div v-else class="mt-5 flex flex-col gap-2.5">
      <ServiceRow v-for="service in store.services" :key="service.id" :service="service" />
    </div>

    <BusyOverlay
      :show="bulk !== null"
      :label="busyLabel"
      detail="The rows update as each comes up."
    />
  </section>
</template>
