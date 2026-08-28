<script setup lang="ts">
import { useServicesStore } from '@/stores/services'

const servicesStore = useServicesStore()
</script>

<template>
  <section>
    <h1 class="text-2xl font-semibold text-neutral-900 dark:text-neutral-100">Dashboard</h1>
    <p class="mt-1 text-sm text-neutral-500">Manage your local development services.</p>

    <div v-if="servicesStore.services.length === 0" class="mt-6 text-sm text-neutral-500">
      No services detected yet.
    </div>
  </section>
</template>
