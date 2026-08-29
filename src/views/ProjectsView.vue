<script setup lang="ts">
import { useProjectsStore } from '@/stores/projects'

const store = useProjectsStore()
</script>

<template>
  <section>
    <h1 class="text-[28px] leading-tight font-bold tracking-tight">Projects</h1>
    <p class="mt-1 text-sm text-neutral-500">Local projects detected on this machine.</p>

    <div v-if="store.projects.length === 0" class="mt-6 text-sm text-neutral-500">
      No projects found yet.
    </div>
    <div v-else class="mt-5 flex flex-col gap-2.5">
      <div
        v-for="project in store.projects"
        :key="project.id"
        class="flex items-center gap-3 rounded-2xl border border-neutral-200/80 bg-neutral-100/60 p-3.5 dark:border-neutral-800 dark:bg-neutral-900/60"
      >
        <span
          class="flex h-9 w-9 shrink-0 items-center justify-center rounded-full bg-neutral-200/70 text-neutral-500 dark:bg-neutral-800 dark:text-neutral-400"
        >
          <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            class="h-4 w-4"
          >
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              d="M3 7a2 2 0 0 1 2-2h3.6l2 2.5H19a2 2 0 0 1 2 2V17a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z"
            />
          </svg>
        </span>

        <div class="min-w-0 flex-1">
          <p class="truncate font-semibold text-neutral-900 dark:text-neutral-100">
            {{ project.name }}
          </p>
          <p class="truncate font-mono text-xs text-neutral-500">{{ project.path }}</p>
        </div>

        <span
          class="shrink-0 rounded-full bg-white/70 px-2.5 py-0.5 font-mono text-xs text-neutral-500 dark:bg-neutral-800 dark:text-neutral-400"
        >
          {{ project.domain }}
        </span>
      </div>
    </div>
  </section>
</template>
