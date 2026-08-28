<script setup lang="ts">
import { useServicesStore } from '@/stores/services'
import ServiceRow from '@/components/services/ServiceRow.vue'

const store = useServicesStore()
</script>

<template>
  <section>
    <div class="flex items-start justify-between gap-4">
      <div>
        <h1 class="text-2xl font-semibold text-neutral-900 dark:text-neutral-100">Services</h1>
        <p class="mt-1 text-sm text-neutral-500">Your local stack, one tap away.</p>
      </div>

      <div class="flex items-center gap-2">
        <button
          type="button"
          class="rounded-lg bg-red-600 px-4 py-2 text-sm font-semibold text-white transition hover:bg-red-500"
          @click="store.startAll"
        >
          Start all
        </button>
        <button
          type="button"
          class="rounded-lg border border-neutral-200 px-4 py-2 text-sm font-semibold text-neutral-700 transition hover:bg-neutral-100 dark:border-neutral-700 dark:text-neutral-200 dark:hover:bg-neutral-800"
          @click="store.stopAll"
        >
          Stop all
        </button>
      </div>
    </div>

    <div v-if="store.loading && store.services.length === 0" class="mt-6 text-sm text-neutral-500">
      Loading services…
    </div>
    <div v-else class="mt-6 flex flex-col gap-3">
      <ServiceRow v-for="service in store.services" :key="service.id" :service="service" />
    </div>
  </section>
</template>
