<script setup lang="ts">
import { computed, ref } from 'vue'
import { useProjectsStore } from '@/stores/projects'
import SearchInput from '@/components/common/SearchInput.vue'
import BasePill from '@/components/common/BasePill.vue'
import ProjectActionButtons from '@/components/projects/ProjectActionButtons.vue'
import NewProjectModal from '@/components/projects/NewProjectModal.vue'

const store = useProjectsStore()

const search = ref('')
const viewMode = ref<'grid' | 'list'>('list')
const showNewProjectModal = ref(false)

const filteredProjects = computed(() => {
  const query = search.value.trim().toLowerCase()
  if (!query) return store.projects
  return store.projects.filter(
    (project) =>
      project.name.toLowerCase().includes(query) || project.domain.toLowerCase().includes(query),
  )
})

const TOGGLE_BUTTON_CLASS =
  'flex items-center gap-1.5 rounded-full px-3.5 py-1.5 text-sm font-semibold transition'

function toggleClass(mode: 'grid' | 'list') {
  return viewMode.value === mode
    ? 'bg-red-100 text-red-600 dark:bg-red-500/15 dark:text-red-400'
    : 'text-neutral-500 hover:text-neutral-800 dark:text-neutral-400 dark:hover:text-neutral-100'
}

/** An unresolved domain is shown muted rather than in the usual red — the
 *  link colour is a promise that clicking it reaches the site, and until
 *  the hosts file has the entry, it doesn't. */
function domainClass(hasHostsEntry: boolean) {
  return hasHostsEntry ? 'text-red-600 dark:text-red-400' : 'text-neutral-400 dark:text-neutral-500'
}

function domainTitle(hasHostsEntry: boolean) {
  return hasHostsEntry
    ? 'Resolves in the browser'
    : "Not in the hosts file yet — it won't resolve until you sync"
}
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

      <div class="flex shrink-0 items-center gap-2">
        <button
          type="button"
          class="flex items-center gap-2 rounded-full border border-neutral-200 bg-white/70 px-4 py-2.5 text-sm font-semibold text-neutral-700 transition hover:bg-white disabled:opacity-50 dark:border-neutral-700 dark:bg-neutral-900/60 dark:text-neutral-200 dark:hover:bg-neutral-800"
          :disabled="store.syncingHosts || store.unresolvedProjects.length === 0"
          :title="
            store.unresolvedProjects.length === 0
              ? 'Every domain already resolves'
              : 'Add project domains to the hosts file (needs admin approval)'
          "
          @click="store.syncHosts"
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
              d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15"
            />
          </svg>
          {{ store.syncingHosts ? 'Waiting for admin approval…' : 'Sync hosts file' }}
        </button>

        <button
          type="button"
          class="flex items-center gap-2 rounded-full bg-red-600 px-5 py-2.5 text-sm font-semibold text-white shadow-lg shadow-red-500/40 transition hover:bg-red-500"
          @click="showNewProjectModal = true"
        >
          <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2.5"
            class="h-4 w-4"
          >
            <path stroke-linecap="round" d="M12 5v14M5 12h14" />
          </svg>
          New project
        </button>
      </div>
    </div>

    <p v-if="store.hostsError" class="mt-3 text-sm text-red-600 dark:text-red-400">
      {{ store.hostsError }}
    </p>
    <p v-if="store.openError" class="mt-3 text-sm text-red-600 dark:text-red-400">
      {{ store.openError }}
    </p>

    <NewProjectModal v-if="showNewProjectModal" @close="showNewProjectModal = false" />

    <div class="mt-5 flex flex-wrap items-center gap-3">
      <SearchInput
        v-model="search"
        placeholder="Search projects or domains"
        class="w-full max-w-md"
      />

      <div
        class="flex shrink-0 items-center gap-0.5 rounded-full border border-neutral-200 bg-white/70 p-1 dark:border-neutral-700 dark:bg-neutral-900/60"
      >
        <button
          type="button"
          :class="[TOGGLE_BUTTON_CLASS, toggleClass('list')]"
          @click="viewMode = 'list'"
        >
          <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            class="h-3.5 w-3.5"
          >
            <path stroke-linecap="round" d="M4 6h16M4 12h16M4 18h16" />
          </svg>
          List
        </button>
        <button
          type="button"
          :class="[TOGGLE_BUTTON_CLASS, toggleClass('grid')]"
          @click="viewMode = 'grid'"
        >
          <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            class="h-3.5 w-3.5"
          >
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
      <template v-if="store.projects.length === 0">
        No projects yet — create one, or drop a folder into your www directory.
      </template>
      <template v-else>No projects match "{{ search }}".</template>
    </div>

    <div
      v-else-if="viewMode === 'grid'"
      class="mt-5 grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-3"
    >
      <div
        v-for="project in filteredProjects"
        :key="project.id"
        class="rounded-2xl border border-neutral-200 bg-white p-4 transition hover:border-neutral-300 dark:border-neutral-800 dark:bg-neutral-900/60 dark:hover:border-neutral-700"
      >
        <div class="flex items-start justify-between gap-2">
          <p class="truncate font-semibold text-neutral-900 dark:text-neutral-100">
            {{ project.name }}
          </p>
          <BasePill class="shrink-0">{{ project.stack }}</BasePill>
        </div>
        <p class="mt-1 truncate font-mono text-xs text-neutral-500">{{ project.path }}</p>

        <div
          class="mt-3 flex items-center justify-between gap-2 border-t border-neutral-200/80 pt-3 dark:border-neutral-800"
        >
          <span
            class="min-w-0 truncate rounded-full px-3 py-1.5 font-mono text-xs"
            :class="
              project.hasHostsEntry
                ? 'bg-red-50 text-red-600 dark:bg-red-500/10 dark:text-red-400'
                : 'bg-neutral-100 text-neutral-400 dark:bg-neutral-800 dark:text-neutral-500'
            "
            :title="domainTitle(project.hasHostsEntry)"
          >
            {{ project.domain }}
          </span>
          <ProjectActionButtons
            :project-id="project.id"
            :domain="project.domain"
            :path="project.path"
          />
        </div>
      </div>
    </div>

    <div
      v-else
      class="mt-5 overflow-hidden rounded-2xl border border-neutral-200 bg-white dark:border-neutral-800 dark:bg-neutral-900/60"
    >
      <div
        class="flex items-center gap-3 border-b border-neutral-200 bg-neutral-50/80 px-5 py-3 text-[11px] font-semibold tracking-wide text-neutral-400 uppercase dark:border-neutral-800 dark:bg-neutral-900/40"
      >
        <span class="flex-1">Project</span>
        <span class="w-44 shrink-0">Domain</span>
        <span class="w-28 shrink-0">Stack</span>
        <span class="w-44 shrink-0 text-right">Actions</span>
      </div>

      <div
        v-for="project in filteredProjects"
        :key="project.id"
        class="flex items-center gap-3 border-b border-neutral-200/70 px-5 py-3.5 transition last:border-b-0 hover:bg-neutral-50 dark:border-neutral-800/70 dark:hover:bg-neutral-800/30"
      >
        <div class="min-w-0 flex-1">
          <p class="truncate font-semibold text-neutral-900 dark:text-neutral-100">
            {{ project.name }}
          </p>
          <p class="truncate font-mono text-xs text-neutral-500">{{ project.path }}</p>
        </div>
        <span
          class="w-44 shrink-0 truncate font-mono text-xs"
          :class="domainClass(project.hasHostsEntry)"
          :title="domainTitle(project.hasHostsEntry)"
        >
          {{ project.domain }}
        </span>
        <span class="w-28 shrink-0">
          <BasePill>{{ project.stack }}</BasePill>
        </span>
        <div class="flex w-44 shrink-0 justify-end">
          <ProjectActionButtons
            :project-id="project.id"
            :domain="project.domain"
            :path="project.path"
          />
        </div>
      </div>
    </div>
  </section>
</template>
