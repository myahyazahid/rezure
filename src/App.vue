<script setup lang="ts">
import { RouterView } from 'vue-router'
import { onMounted } from 'vue'
import AppTitleBar from '@/components/common/AppTitleBar.vue'
import AppSidebar from '@/components/common/AppSidebar.vue'
import { useServicesStore } from '@/stores/services'
import { useProjectsStore } from '@/stores/projects'
import { usePhpStore } from '@/stores/php'
import { useBinariesStore } from '@/stores/binaries'
import { useDatabasesStore } from '@/stores/databases'

const servicesStore = useServicesStore()
const projectsStore = useProjectsStore()
const phpStore = usePhpStore()
const binariesStore = useBinariesStore()
const databasesStore = useDatabasesStore()

onMounted(() => {
  // Sidebar badges and the dashboard read from all of these, so they are loaded up front.
  servicesStore.fetchAll()
  projectsStore.fetchAll()
  phpStore.fetchAll()
  binariesStore.fetchAll()
  // Best-effort: this one fails when MariaDB isn't running, which the
  // Databases page reports on its own rather than as a startup error.
  databasesStore.fetchAll()
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
