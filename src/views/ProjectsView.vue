<script setup lang="ts">
import { computed, ref } from 'vue'
import { useProjectsStore } from '@/stores/projects'
import SearchInput from '@/components/common/SearchInput.vue'
import BasePill from '@/components/common/BasePill.vue'
import ProjectActionButtons from '@/components/projects/ProjectActionButtons.vue'
import NewProjectModal from '@/components/projects/NewProjectModal.vue'
import LinkProjectModal from '@/components/projects/LinkProjectModal.vue'
import ProjectDoctorModal from '@/components/projects/ProjectDoctorModal.vue'
import BusyOverlay from '@/components/common/BusyOverlay.vue'

const store = useProjectsStore()

/**
 * The three slow paths on this page, most disruptive first.
 *
 * Scaffolding is by far the longest — Composer resolves and downloads a whole
 * dependency tree over the network — so it leads. The hosts sync is last
 * because Windows puts its own consent dialog on top anyway; the overlay is
 * there for the stretch before and after that prompt.
 */
const busy = computed(() => {
  if (store.creating) {
    return {
      label: 'Creating project…',
      detail: 'Composer is downloading dependencies — this can take a few minutes.',
    }
  }
  if (store.linking) {
    return { label: 'Adding project…', detail: 'Writing its vhost and reloading nginx.' }
  }
  if (store.syncingHosts) {
    return {
      label: 'Waiting for admin approval…',
      detail: 'Windows is asking permission to edit the hosts file.',
    }
  }
  return null
})

const search = ref('')
const viewMode = ref<'grid' | 'list'>('list')
const showNewProjectModal = ref(false)
const showLinkProjectModal = ref(false)
/** Unlink asks first — it's a list the user curated, and an accidental
 *  click would otherwise silently drop an entry. */
const confirmingUnlink = ref<string | null>(null)

async function unlink(id: string) {
  confirmingUnlink.value = null
  await store.unlinkProject(id)
}

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

/** `lastOpenedAt` is unix seconds from the backend; `null` until a project
 *  has been opened (site, folder, or terminal) at least once. */
function lastOpenedLabel(project: { lastOpenedAt: number | null; openCount: number }) {
  if (project.lastOpenedAt === null) return 'Not opened yet'
  const diffMs = Date.now() - project.lastOpenedAt * 1000
  const days = Math.floor(diffMs / 86_400_000)
  const when = days <= 0 ? 'today' : days === 1 ? '1 day ago' : `${days} days ago`
  return `Opened ${project.openCount}× · last ${when}`
}
</script>

<template>
  <!-- Fills the main area exactly and scrolls the list inside itself, rather
       than growing and letting the whole page scroll. With enough projects
       the header, search and view toggle would otherwise scroll off the top
       just when they're most needed. -->
  <section class="flex h-full flex-col">
    <div class="flex shrink-0 items-start justify-between gap-4">
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
          class="flex items-center gap-2 rounded-full border border-neutral-200 bg-white/70 px-4 py-2.5 text-sm font-semibold text-neutral-700 transition hover:bg-white dark:border-neutral-700 dark:bg-neutral-900/60 dark:text-neutral-200 dark:hover:bg-neutral-800"
          title="Serve a project from a folder outside your www directory"
          @click="showLinkProjectModal = true"
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
              d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"
            />
          </svg>
          Add folder
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
    <LinkProjectModal v-if="showLinkProjectModal" @close="showLinkProjectModal = false" />

    <!-- Rendered once, outside both the card grid and the list: the result
         is about one project at a time, and the store already says which. -->
    <ProjectDoctorModal />

    <BusyOverlay :show="busy !== null" :label="busy?.label ?? ''" :detail="busy?.detail ?? ''" />

    <!-- Says plainly that nothing is deleted. "Remove" next to a folder path
         reads as destructive unless the opposite is stated. -->
    <div
      v-if="confirmingUnlink"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
      @click.self="confirmingUnlink = null"
    >
      <div
        class="w-full max-w-md rounded-2xl border border-neutral-200 bg-white p-6 shadow-2xl dark:border-neutral-700 dark:bg-neutral-900"
      >
        <h2 class="text-lg font-bold text-neutral-900 dark:text-neutral-100">
          Remove this project from Rezure?
        </h2>
        <p class="mt-2 text-sm text-neutral-500">
          Rezure stops serving it and drops its virtual host.
          <strong class="text-neutral-700 dark:text-neutral-200">
            The folder and everything in it stays exactly where it is
          </strong>
          — you can add it again any time.
        </p>
        <div class="mt-6 flex justify-end gap-2">
          <button
            type="button"
            class="rounded-full px-4 py-2 text-sm font-semibold text-neutral-600 dark:text-neutral-300"
            @click="confirmingUnlink = null"
          >
            Cancel
          </button>
          <button
            type="button"
            class="rounded-full bg-red-600 px-5 py-2 text-sm font-semibold text-white transition hover:bg-red-500"
            @click="unlink(confirmingUnlink)"
          >
            Remove
          </button>
        </div>
      </div>
    </div>

    <p v-if="store.linkError" class="mt-3 text-sm text-red-600 dark:text-red-400">
      {{ store.linkError }}
    </p>

    <div class="mt-5 flex shrink-0 flex-wrap items-center gap-3">
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

    <div
      v-if="filteredProjects.length === 0"
      class="mt-8 shrink-0 text-center text-sm text-neutral-500"
    >
      <template v-if="store.projects.length === 0">
        No projects yet — create one, or drop a folder into your www directory.
      </template>
      <template v-else>No projects match "{{ search }}".</template>
    </div>

    <div
      v-else-if="viewMode === 'grid'"
      class="mt-5 grid min-h-0 flex-1 grid-cols-1 content-start gap-3 overflow-y-auto pr-1 sm:grid-cols-2 xl:grid-cols-3"
    >
      <div
        v-for="project in filteredProjects"
        :key="project.id"
        class="rounded-2xl border border-neutral-200 bg-white p-4 transition hover:border-neutral-300 dark:border-neutral-800 dark:bg-neutral-900/60 dark:hover:border-neutral-700"
      >
        <div class="flex items-start justify-between gap-2">
          <p
            class="flex min-w-0 items-center gap-2 truncate font-semibold text-neutral-900 dark:text-neutral-100"
          >
            <span class="truncate">{{ project.name }}</span>
            <span
              v-if="project.kind === 'linked'"
              class="shrink-0 rounded-full bg-neutral-100 px-2 py-0.5 text-[10px] font-semibold tracking-wide text-neutral-500 uppercase dark:bg-neutral-800 dark:text-neutral-400"
              title="Served from a folder outside your www directory"
            >
              Linked
            </span>
          </p>
          <BasePill class="shrink-0">{{ project.stack }}</BasePill>
        </div>
        <p class="mt-1 truncate font-mono text-xs text-neutral-500">{{ project.path }}</p>
        <p
          v-if="project.missing"
          class="mt-0.5 truncate text-xs text-amber-600 dark:text-amber-400"
        >
          Folder not found — it may have moved, or be on a drive that isn't connected.
        </p>
        <p v-else-if="lastOpenedLabel(project)" class="mt-0.5 truncate text-xs text-neutral-400">
          {{ lastOpenedLabel(project) }}
        </p>

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
            v-if="!project.missing"
            :project-id="project.id"
            :domain="project.domain"
            :path="project.path"
          />
          <button
            v-if="project.kind === 'linked'"
            type="button"
            title="Remove from Rezure (the folder is left alone)"
            class="flex h-9 w-9 shrink-0 items-center justify-center rounded-full border border-neutral-200 text-neutral-400 transition hover:border-red-300 hover:text-red-600 dark:border-neutral-700 dark:hover:border-red-500/40 dark:hover:text-red-400"
            @click="confirmingUnlink = project.id"
          >
            <svg
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
              class="h-4 w-4"
            >
              <path stroke-linecap="round" d="M18.4 5.6 5.6 18.4M5.6 5.6l12.8 12.8" />
            </svg>
          </button>
        </div>
      </div>
    </div>

    <div
      v-else
      class="mt-5 flex min-h-0 flex-1 flex-col overflow-hidden rounded-2xl border border-neutral-200 bg-white dark:border-neutral-800 dark:bg-neutral-900/60"
    >
      <!-- Column headings stay put; only the rows below them move. -->
      <div
        class="flex shrink-0 items-center gap-3 border-b border-neutral-200 bg-neutral-50/80 px-5 py-3 text-[11px] font-semibold tracking-wide text-neutral-400 uppercase dark:border-neutral-800 dark:bg-neutral-900/40"
      >
        <span class="flex-1">Project</span>
        <span class="w-44 shrink-0">Domain</span>
        <span class="w-28 shrink-0">Stack</span>
        <span class="w-56 shrink-0 text-right">Actions</span>
      </div>

      <div class="min-h-0 flex-1 overflow-y-auto">
        <div
          v-for="project in filteredProjects"
          :key="project.id"
          class="flex items-center gap-3 border-b border-neutral-200/70 px-5 py-3.5 transition last:border-b-0 hover:bg-neutral-50 dark:border-neutral-800/70 dark:hover:bg-neutral-800/30"
        >
          <div class="min-w-0 flex-1">
            <p
              class="flex items-center gap-2 truncate font-semibold text-neutral-900 dark:text-neutral-100"
            >
              {{ project.name }}
              <!-- Only linked projects carry a badge: a project in www is the
                 norm and doesn't need labelling. -->
              <span
                v-if="project.kind === 'linked'"
                class="shrink-0 rounded-full bg-neutral-100 px-2 py-0.5 text-[10px] font-semibold tracking-wide text-neutral-500 uppercase dark:bg-neutral-800 dark:text-neutral-400"
                title="Served from a folder outside your www directory"
              >
                Linked
              </span>
            </p>
            <p class="truncate font-mono text-xs text-neutral-500">{{ project.path }}</p>
            <!-- Always three lines, so every row is the same height. Leaving
                 this one out for never-opened projects made the list ragged,
                 and rows changed height the first time you opened one. -->
            <p v-if="project.missing" class="truncate text-xs text-amber-600 dark:text-amber-400">
              Folder not found — it may have moved, or be on a drive that isn't connected.
            </p>
            <p v-else class="truncate text-xs text-neutral-400">
              {{ lastOpenedLabel(project) }}
            </p>
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
          <!-- Every control keeps its column across all rows. The buttons used
               to be dropped rather than hidden, so a row without an unlink
               button pushed Open to the right and the actions never lined up
               down the list. -->
          <div class="flex w-56 shrink-0 items-center justify-end gap-1">
            <ProjectActionButtons
              :class="project.missing ? 'invisible' : ''"
              :project-id="project.id"
              :domain="project.domain"
              :path="project.path"
            />
            <!-- Unlink only exists for linked projects: a scanned one is
               removed by moving its folder out of www, not from here. The
               empty slot keeps the button column aligned on the other rows.
               `h-9 w-9` matches its siblings — it used to be a size smaller. -->
            <button
              v-if="project.kind === 'linked'"
              type="button"
              title="Remove from Rezure (the folder is left alone)"
              class="flex h-9 w-9 shrink-0 items-center justify-center rounded-full border border-neutral-200 text-neutral-400 transition hover:border-red-300 hover:text-red-600 dark:border-neutral-700 dark:hover:border-red-500/40 dark:hover:text-red-400"
              @click="confirmingUnlink = project.id"
            >
              <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                class="h-4 w-4"
              >
                <path stroke-linecap="round" d="M18.4 5.6 5.6 18.4M5.6 5.6l12.8 12.8" />
              </svg>
            </button>
            <span v-else class="h-9 w-9 shrink-0" aria-hidden="true"></span>
          </div>
        </div>
      </div>
    </div>
  </section>
</template>
