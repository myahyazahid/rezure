<script setup lang="ts">
import { useProjectsStore } from '@/stores/projects'

const projectsStore = useProjectsStore()
</script>

<template>
  <section>
    <h1 class="text-2xl font-semibold text-neutral-900 dark:text-neutral-100">Projects</h1>
    <p class="mt-1 text-sm text-neutral-500">Local projects detected on this machine.</p>

    <div v-if="projectsStore.projects.length === 0" class="mt-6 text-sm text-neutral-500">
      No projects found yet.
    </div>
  </section>
</template>
