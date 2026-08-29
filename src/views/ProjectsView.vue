<script setup lang="ts">
import { computed, ref } from 'vue'
import { useProjectsStore } from '@/stores/projects'
import SearchInput from '@/components/common/SearchInput.vue'
import BasePill from '@/components/common/BasePill.vue'
import ProjectActionButtons from '@/components/projects/ProjectActionButtons.vue'

const store = useProjectsStore()

const search = ref('')
const viewMode = ref<'grid' | 'list'>('grid')

const filteredProjects = computed(() => {
  const query = search.value.trim().toLowerCase()
  if (!query) return store.projects
  return store.projects.filter(
    (project) =>
      project.name.toLowerCase().includes(query) || project.domain.toLowerCase().includes(query),
  )
})
</script>

<template>
  <section>
    <div class="flex items-start justify-between gap-4">
      <div>
        <h1 class="text-[28px] leading-tight font-bold tracking-tight">Projects</h1>
        <p class="mt-1 text-sm text-neutral-500">
          {{ store.projects.length }} local sites, auto-served with their own domain.
        </p>
      </div>

      <button
        type="button"
        class="flex shrink-0 items-center gap-2 rounded-full bg-red-600 px-5 py-2.5 text-sm font-semibold text-white shadow-lg shadow-red-500/40 transition hover:bg-red-500"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" class="h-4 w-4">
          <path stroke-linecap="round" d="M12 5v14M5 12h14" />
        </svg>
        New project
      </button>
    </div>

    <div class="mt-5 flex flex-wrap items-center gap-2">
      <SearchInput v-model="search" placeholder="Search projects or domains" class="min-w-55 flex-1" />

      <div
        class="flex shrink-0 items-center gap-0.5 rounded-full border border-neutral-200 bg-white/70 p-1 dark:border-neutral-700 dark:bg-neutral-900/60"
      >
        <button
          type="button"
          class="flex items-center gap-1.5 rounded-full px-3 py-1.5 text-sm font-semibold transition"
          :class="
            viewMode === 'list'
              ? 'bg-red-100 text-red-600 dark:bg-red-500/15 dark:text-red-400'
              : 'text-neutral-500 hover:text-neutral-800 dark:text-neutral-400 dark:hover:text-neutral-100'
          "
          @click="viewMode = 'list'"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="h-3.5 w-3.5">
            <path stroke-linecap="round" d="M4 6h16M4 12h16M4 18h16" />
          </svg>
          List
        </button>
        <button
          type="button"
          class="flex items-center gap-1.5 rounded-full px-3 py-1.5 text-sm font-semibold transition"
          :class="
            viewMode === 'grid'
              ? 'bg-red-100 text-red-600 dark:bg-red-500/15 dark:text-red-400'
              : 'text-neutral-500 hover:text-neutral-800 dark:text-neutral-400 dark:hover:text-neutral-100'
          "
          @click="viewMode = 'grid'"
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="h-3.5 w-3.5">
            <rect x="3.5" y="3.5" width="7" height="7" rx="1.5" />
            <rect x="13.5" y="3.5" width="7" height="7" rx="1.5" />
            <rect x="3.5" y="13.5" width="7" height="7" rx="1.5" />
            <rect x="13.5" y="13.5" width="7" height="7" rx="1.5" />
          </svg>
          Grid
        </button>
      </div>

      <span class="shrink-0 text-sm text-neutral-500">{{ filteredProjects.length }} projects</span>
    </div>

    <div v-if="filteredProjects.length === 0" class="mt-8 text-center text-sm text-neutral-500">
      No projects match "{{ search }}".
    </div>

    <div
      v-else-if="viewMode === 'grid'"
      class="mt-5 grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-3"
    >
      <div
        v-for="project in filteredProjects"
        :key="project.id"
        class="rounded-2xl border border-neutral-200/80 bg-neutral-100/60 p-4 transition hover:border-neutral-300 dark:border-neutral-800 dark:bg-neutral-900/60"
      >
        <div class="flex items-start justify-between gap-2">
          <p class="truncate font-semibold text-neutral-900 dark:text-neutral-100">
            {{ project.name }}
          </p>
          <BasePill class="shrink-0">{{ project.stack }}</BasePill>
        </div>
        <p class="mt-1 truncate font-mono text-xs text-neutral-500">{{ project.path }}</p>

        <div class="mt-4 flex items-center justify-between gap-2">
          <span
            class="truncate rounded-full bg-red-50 px-2.5 py-1 font-mono text-xs text-red-600 dark:bg-red-500/10 dark:text-red-400"
          >
            {{ project.domain }}
          </span>
          <ProjectActionButtons :domain="project.domain" :path="project.path" />
        </div>
      </div>
    </div>

    <div
      v-else
      class="mt-5 overflow-hidden rounded-2xl border border-neutral-200/80 bg-neutral-100/60 dark:border-neutral-800 dark:bg-neutral-900/60"
    >
      <div
        class="flex items-center gap-3 border-b border-neutral-200/80 px-4 py-2.5 text-xs font-semibold tracking-wide text-neutral-400 uppercase dark:border-neutral-800"
      >
        <span class="flex-1">Project</span>
        <span class="w-40 shrink-0">Domain</span>
        <span class="w-28 shrink-0">Stack</span>
        <span class="w-26 shrink-0">Actions</span>
      </div>

      <div
        v-for="project in filteredProjects"
        :key="project.id"
        class="flex items-center gap-3 border-b border-neutral-200/60 px-4 py-3 last:border-b-0 dark:border-neutral-800/60"
      >
        <div class="min-w-0 flex-1">
          <p class="truncate font-semibold text-neutral-900 dark:text-neutral-100">
            {{ project.name }}
          </p>
          <p class="truncate font-mono text-xs text-neutral-500">{{ project.path }}</p>
        </div>
        <span class="w-40 shrink-0 truncate font-mono text-xs text-red-600 dark:text-red-400">
          {{ project.domain }}
        </span>
        <span class="w-28 shrink-0">
          <BasePill>{{ project.stack }}</BasePill>
        </span>
        <span class="w-26 shrink-0">
          <ProjectActionButtons :domain="project.domain" :path="project.path" />
        </span>
      </div>
    </div>
  </section>
</template>
