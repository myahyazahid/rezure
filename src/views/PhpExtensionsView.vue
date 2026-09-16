<script setup lang="ts">
import { computed, onActivated, ref, watch } from 'vue'
import { RouterLink } from 'vue-router'
import { usePhpStore } from '@/stores/php'
import { useServicesStore } from '@/stores/services'
import type { BundledExtension } from '@/types/php'
import SearchInput from '@/components/common/SearchInput.vue'

const store = usePhpStore()
const servicesStore = useServicesStore()

const search = ref('')
/** Which installed version's extensions this page is showing — defaults to
 *  the active one, but several can run at once (per-project pins), so it's
 *  pickable rather than always following the Switch page's active version. */
const selectedVersion = ref<string | null>(null)
const versionMenuOpen = ref(false)

function pickVersion(version: string) {
  selectedVersion.value = version
  versionMenuOpen.value = false
}

function loadFor(version: string | null) {
  if (version) store.fetchBundledExtensions(version)
}

watch(selectedVersion, loadFor)

// Kept-alive view: fires on first mount and on every return to the page,
// same as `SwitchView`'s own PHP fetches — a version installed or pinned
// elsewhere should show up without needing a second visit to notice it.
onActivated(() => {
  if (!selectedVersion.value) selectedVersion.value = store.active?.version ?? null
  loadFor(selectedVersion.value)
})

// The active version can only be known once `store.versions` has loaded,
// which may resolve after this page already activated — this catches that
// without ever overriding a version the user picked from the dropdown.
watch(
  () => store.active?.version ?? null,
  (version) => {
    if (!selectedVersion.value && version) selectedVersion.value = version
  },
)

/** A service actually running this version right now — either the default
 *  `php` service when it's the active one, or a pinned pooled instance
 *  (`services::php_pool`). A toggle here only ever writes state; this is
 *  what decides whether the "restart to apply" note is worth showing. */
const runningThisVersion = computed(
  () =>
    selectedVersion.value !== null &&
    servicesStore.services.some(
      (s) => s.version === selectedVersion.value && s.status === 'running',
    ),
)

const filtered = computed(() => {
  const query = search.value.trim().toLowerCase()
  if (!query) return store.bundledExtensions
  return store.bundledExtensions.filter(
    (ext) => ext.label.toLowerCase().includes(query) || ext.id.toLowerCase().includes(query),
  )
})

/** Grouped so the page reads as a handful of short sections instead of one
 *  long alphabetical wall — categories and, within one, extensions both
 *  sorted for a stable order across re-renders. */
const groups = computed(() => {
  const byCategory = new Map<string, BundledExtension[]>()
  for (const ext of filtered.value) {
    const list = byCategory.get(ext.category) ?? []
    list.push(ext)
    byCategory.set(ext.category, list)
  }
  return [...byCategory.entries()]
    .map(
      ([category, exts]) =>
        [category, [...exts].sort((a, b) => a.label.localeCompare(b.label))] as const,
    )
    .sort(([a], [b]) => a.localeCompare(b))
})

async function toggle(ext: BundledExtension) {
  if (!selectedVersion.value || !ext.available) return
  await store.setBundledExtension(selectedVersion.value, ext.id, !ext.enabled)
}
</script>

<template>
  <section>
    <div class="flex flex-wrap items-start justify-between gap-4">
      <div>
        <h1 class="text-[28px] leading-tight font-bold tracking-tight">PHP Extensions</h1>
        <p class="mt-1 text-sm text-neutral-500">
          Turn on extensions this PHP zip already ships in <code class="font-mono">ext/</code> — no
          download, no hand-edited <code class="font-mono">.ini</code> fragment.
        </p>
      </div>

      <div v-if="store.versions.length > 1" class="relative shrink-0">
        <button
          type="button"
          class="flex items-center gap-1.5 rounded-full border border-neutral-200 bg-white px-4 py-2 text-sm font-semibold text-neutral-700 transition hover:bg-neutral-50 dark:border-neutral-700 dark:bg-neutral-900/60 dark:text-neutral-200 dark:hover:bg-neutral-800"
          @click="versionMenuOpen = !versionMenuOpen"
        >
          PHP {{ selectedVersion }}
          <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            class="h-3.5 w-3.5 shrink-0 transition-transform"
            :class="versionMenuOpen ? 'rotate-180' : ''"
          >
            <path stroke-linecap="round" stroke-linejoin="round" d="m6 9 6 6 6-6" />
          </svg>
        </button>

        <template v-if="versionMenuOpen">
          <div class="fixed inset-0 z-10" @click="versionMenuOpen = false"></div>
          <div
            class="absolute top-full right-0 z-20 mt-2 w-40 rounded-xl border border-neutral-200 bg-white p-1 shadow-lg dark:border-neutral-700 dark:bg-neutral-900"
          >
            <button
              v-for="v in store.versions"
              :key="v.id"
              type="button"
              class="flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-sm transition hover:bg-neutral-100 dark:hover:bg-neutral-800"
              @click="pickVersion(v.version)"
            >
              <span
                class="h-1.5 w-1.5 shrink-0 rounded-full"
                :class="v.version === selectedVersion ? 'bg-red-500' : 'bg-transparent'"
              />
              <span class="flex-1 truncate">PHP {{ v.version }}</span>
            </button>
          </div>
        </template>
      </div>
    </div>

    <p v-if="store.error" class="mt-3 text-sm text-red-600 dark:text-red-400">
      {{ store.error }}
    </p>

    <p
      v-if="store.versions.length === 0"
      class="mt-6 rounded-2xl border border-neutral-200/80 bg-neutral-100/60 p-6 text-center text-sm text-neutral-500 dark:border-neutral-800 dark:bg-neutral-900/60"
    >
      No PHP version installed yet — install one from
      <RouterLink to="/switch" class="font-semibold text-red-600 hover:underline"
        >Switch</RouterLink
      >
      first.
    </p>

    <template v-else>
      <p
        v-if="runningThisVersion"
        class="mt-4 rounded-xl bg-amber-50 px-3 py-2 text-sm text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
      >
        PHP {{ selectedVersion }} is running right now — restart it (Dashboard → PHP) for a toggle
        here to reach requests it's already serving.
      </p>

      <SearchInput v-model="search" placeholder="Filter extensions" class="mt-4 max-w-sm" />

      <div class="mt-4 flex flex-col gap-3">
        <div v-for="[category, exts] in groups" :key="category">
          <p class="mb-1.5 text-xs font-semibold tracking-wide text-neutral-400 uppercase">
            {{ category }}
          </p>
          <div
            class="flex flex-col divide-y divide-neutral-200/80 rounded-2xl border border-neutral-200/80 bg-neutral-100/60 dark:divide-neutral-800 dark:border-neutral-800 dark:bg-neutral-900/60"
          >
            <div
              v-for="ext in exts"
              :key="ext.id"
              class="flex items-center gap-3 px-3.5 py-2.5"
              :class="{ 'opacity-50': !ext.available }"
            >
              <div class="min-w-0 flex-1">
                <div class="flex flex-wrap items-center gap-1.5">
                  <span class="text-sm font-medium text-neutral-800 dark:text-neutral-200">
                    {{ ext.label }}
                  </span>
                  <code class="text-[11px] text-neutral-400">{{ ext.id }}</code>
                  <span
                    v-if="ext.debugOnly"
                    class="rounded-full bg-amber-100 px-1.5 py-0.5 text-[10px] font-semibold text-amber-700 dark:bg-amber-500/15 dark:text-amber-300"
                    title="Environment-dependent or debug-only — not an ordinary extension."
                  >
                    special
                  </span>
                  <span
                    v-if="!ext.available"
                    class="rounded-full bg-neutral-200 px-1.5 py-0.5 text-[10px] font-semibold text-neutral-500 dark:bg-neutral-700 dark:text-neutral-400"
                  >
                    not in this build
                  </span>
                </div>
                <p class="mt-0.5 truncate text-xs text-neutral-500">{{ ext.description }}</p>
              </div>

              <button
                type="button"
                role="switch"
                :aria-checked="ext.enabled"
                :aria-label="ext.label"
                class="relative h-5 w-9 shrink-0 rounded-full transition disabled:opacity-40"
                :class="ext.enabled ? 'bg-red-600' : 'bg-neutral-200 dark:bg-neutral-700'"
                :disabled="!ext.available || store.togglingExtension !== null"
                @click="toggle(ext)"
              >
                <span
                  class="absolute top-0.5 h-4 w-4 rounded-full bg-white shadow transition"
                  :class="ext.enabled ? 'left-4.5' : 'left-0.5'"
                />
              </button>
            </div>
          </div>
        </div>

        <p v-if="groups.length === 0" class="text-sm text-neutral-500">
          No extension matches "{{ search }}".
        </p>
      </div>
    </template>
  </section>
</template>
