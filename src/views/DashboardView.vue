<script setup lang="ts">
import { useServicesStore } from '@/stores/services'
import ServiceRow from '@/components/services/ServiceRow.vue'

const store = useServicesStore()
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
          class="flex items-center gap-2 rounded-full bg-red-600 px-5 py-2.5 text-sm font-semibold text-white shadow-lg shadow-red-500/40 transition hover:bg-red-500"
          @click="store.startAll"
        >
          <svg viewBox="0 0 10 10" fill="currentColor" aria-hidden="true" class="h-2.5 w-2.5">
            <path d="M1.5 0.8 9 5 1.5 9.2Z" />
          </svg>
          Start all
        </button>
        <button
          type="button"
          class="flex items-center gap-2 rounded-full border border-neutral-200 bg-white/70 px-5 py-2.5 text-sm font-semibold text-neutral-700 transition hover:bg-white dark:border-neutral-700 dark:bg-neutral-900/60 dark:text-neutral-200 dark:hover:bg-neutral-800"
          @click="store.stopAll"
        >
          <svg viewBox="0 0 10 10" fill="currentColor" aria-hidden="true" class="h-2 w-2">
            <rect width="10" height="10" rx="1.5" />
          </svg>
          Stop all
        </button>
      </div>
    </div>

    <div v-if="store.loading && store.services.length === 0" class="mt-6 text-sm text-neutral-500">
      Loading services…
    </div>
    <div v-else class="mt-5 flex flex-col gap-2.5">
      <ServiceRow v-for="service in store.services" :key="service.id" :service="service" />
    </div>
  </section>
</template>
