<script setup lang="ts">
import { computed, ref } from 'vue'
import { useServicesStore } from '@/stores/services'
import ServiceRow from '@/components/services/ServiceRow.vue'
import BusyOverlay from '@/components/common/BusyOverlay.vue'
import LeafLoader from '@/components/common/LeafLoader.vue'
import ManageServicesModal from '@/components/services/ManageServicesModal.vue'

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

const managing = ref(false)

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
  'glass-ghost flex items-center gap-2 rounded-full px-4 py-1.5 text-sm font-semibold transition disabled:opacity-50'
</script>

<template>
  <section>
    <div class="flex items-start justify-between gap-4">
      <div>
        <h1 class="text-[28px] leading-tight font-bold tracking-tight">Services</h1>
        <p class="mt-1 text-sm text-neutral-500">Your local stack, one tap away.</p>
      </div>

      <div class="flex shrink-0 items-center gap-2">
        <!-- Apart from the toolbar: it changes which services are listed,
             not what they're doing. -->
        <button
          type="button"
          class="glass glass-hover flex items-center gap-2 rounded-full px-4 py-2.5 text-sm font-semibold text-neutral-700 transition disabled:opacity-50 dark:text-neutral-100"
          :disabled="bulk !== null"
          title="Choose which services this page shows"
          @click="managing = true"
        >
          <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2.25"
            aria-hidden="true"
            class="h-3.5 w-3.5"
          >
            <path stroke-linecap="round" d="M4 6h9M17 6h3M4 12h3M11 12h9M4 18h11M19 18h1" />
            <circle cx="15" cy="6" r="2" />
            <circle cx="9" cy="12" r="2" />
            <circle cx="17" cy="18" r="2" />
          </svg>
          Manage services
        </button>

        <!-- One toolbar, not three loose buttons: they're the same kind of
           action (every service at once), and grouping them says so. -->
        <div class="glass flex shrink-0 items-center gap-1 rounded-full p-1">
          <button
            type="button"
            class="glass-raised flex items-center gap-2 rounded-full px-4 py-1.5 text-sm font-semibold text-neutral-700 transition disabled:opacity-50 dark:text-neutral-100"
            :disabled="bulk !== null"
            @click="runBulk('start')"
          >
            <svg
              viewBox="0 0 10 10"
              fill="currentColor"
              aria-hidden="true"
              class="h-2.5 w-2.5 text-emerald-500"
            >
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
            <svg
              viewBox="0 0 10 10"
              fill="currentColor"
              aria-hidden="true"
              class="h-2 w-2 text-red-500"
            >
              <rect width="10" height="10" rx="1.5" />
            </svg>
            Stop all
          </button>
        </div>
      </div>
    </div>

    <!-- First load only — a refetch keeps the existing rows on screen. -->
    <div v-if="store.loading && store.services.length === 0" class="mt-12 flex justify-center">
      <LeafLoader :size="52" label="Loading services…" />
    </div>
    <div
      v-else-if="store.services.length === 0"
      class="glass mt-5 flex flex-col items-center gap-3 rounded-2xl px-6 py-10 text-center"
    >
      <p class="text-sm text-neutral-500">No services on this page.</p>
      <button
        type="button"
        class="glass-accent rounded-full px-4 py-2 text-sm font-semibold transition"
        @click="managing = true"
      >
        Add services
      </button>
    </div>
    <div v-else class="mt-5 flex flex-col gap-2.5">
      <ServiceRow v-for="service in store.services" :key="service.id" :service="service" />
    </div>

    <ManageServicesModal v-if="managing" @close="managing = false" />

    <BusyOverlay
      :show="bulk !== null"
      :label="busyLabel"
      detail="The rows update as each comes up."
    />
  </section>
</template>
