<script setup lang="ts">
import { RouterView } from 'vue-router'
import { onMounted } from 'vue'
import AppTitleBar from '@/components/common/AppTitleBar.vue'
import AppSidebar from '@/components/common/AppSidebar.vue'
import { useServicesStore } from '@/stores/services'
import { useProjectsStore } from '@/stores/projects'
import { usePhpStore } from '@/stores/php'

const servicesStore = useServicesStore()
const projectsStore = useProjectsStore()
const phpStore = usePhpStore()

onMounted(() => {
  // Sidebar badges read from all three stores, so they are loaded up front.
  servicesStore.fetchAll()
  projectsStore.fetchAll()
  phpStore.fetchAll()
})
</script>

<template>
  <div
    class="flex h-screen flex-col bg-linear-to-b from-red-50 via-neutral-50 to-neutral-50 text-neutral-900 dark:from-neutral-900 dark:via-neutral-950 dark:to-neutral-950 dark:text-neutral-100"
  >
    <AppTitleBar />

    <div class="flex min-h-0 flex-1">
      <AppSidebar />
      <main class="min-w-0 flex-1 overflow-y-auto px-6 py-5">
        <RouterView />
      </main>
    </div>
  </div>
</template>
