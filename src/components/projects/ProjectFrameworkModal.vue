<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useProjectsStore } from '@/stores/projects'
import { servedFolder, servedFolderLabel } from '@/composables/servedFolder'
import FrameworkPicker from '@/components/projects/FrameworkPicker.vue'
import type { FrameworkChoice, ProjectInfo } from '@/types/project'

const store = useProjectsStore()

const project = computed(() => store.projects.find((p) => p.id === store.frameworkModalFor) ?? null)

/** What the project is set to now, in the picker's terms. */
function currentChoice(p: ProjectInfo | null): FrameworkChoice {
  if (!p || !p.frameworkOverridden) return null
  if (p.framework === 'custom') return { docroot: servedFolder(p.path, p.docroot) }
  return { framework: p.framework }
}

const choice = ref<FrameworkChoice>(null)
/** "Custom folder…" picked with no folder typed yet. */
const choicePending = ref(false)

// Starts from the project's current setting each time it opens; the
// picker's list is only fetched with the Projects page, so make sure it's
// there.
watch(
  () => store.frameworkModalFor,
  (id) => {
    if (!id) return
    choice.value = currentChoice(project.value)
    if (store.frameworkCatalog.frameworks.length === 0) store.fetchFrameworks()
  },
  { immediate: true },
)

const unchanged = computed(
  () => JSON.stringify(choice.value) === JSON.stringify(currentChoice(project.value)),
)

function save() {
  if (!project.value) return
  store.setFramework(project.value.id, choice.value)
}
</script>

<template>
  <div
    v-if="store.frameworkModalFor && project"
    class="glass-scrim fixed inset-0 z-50 flex items-center justify-center p-4"
    @click.self="store.closeFrameworkModal()"
  >
    <div class="glass-strong w-full max-w-lg rounded-2xl p-6">
      <h2 class="text-lg font-bold text-neutral-900 dark:text-neutral-100">
        Framework
        <span class="font-normal text-neutral-400">· {{ project.name }}</span>
      </h2>
      <p class="mt-1 text-sm text-neutral-500">
        Rezure recognises the framework from the project's files, and the framework decides which
        folder the site is served from — <span class="font-mono">public/</span> for Laravel and
        CodeIgniter 4, the project folder for WordPress. If it guessed wrong, pick the right one
        here. The change is live as soon as you save.
      </p>

      <div class="glass-inset mt-4 rounded-xl p-3.5 text-xs text-neutral-500">
        Served as
        <strong class="text-neutral-700 dark:text-neutral-200">{{ project.stack }}</strong>
        <template v-if="project.serve === 'proxy'">
          through its dev server<template v-if="project.devPort">
            on port <span class="font-mono">{{ project.devPort }}</span></template
          >
        </template>
        <template v-else>
          from
          <span class="font-mono">{{ servedFolderLabel(project.path, project.docroot) }}</span>
        </template>
        <template v-if="project.frameworkOverridden">
          · set manually, detection says
          {{ store.frameworkLabel(project.detectedFramework) }}
        </template>
        <template v-else> · detected from its files</template>
      </div>

      <div class="mt-4">
        <FrameworkPicker
          v-model="choice"
          :detected="project.detectedFramework"
          :disabled="store.settingFramework"
          @pending="choicePending = $event"
        />
      </div>

      <p
        v-if="store.frameworkError"
        class="mt-4 rounded-xl bg-amber-100/50 px-3 py-2 text-sm text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
      >
        {{ store.frameworkError }}
      </p>

      <div class="mt-6 flex flex-wrap items-center justify-between gap-2">
        <!-- Edits to etc\frameworks.json are only read when asked for — the
             same config-is-loaded-once rule as everything else in etc\. -->
        <button
          type="button"
          class="rounded-full px-3 py-2 text-xs font-semibold text-neutral-500 transition hover:text-neutral-800 disabled:opacity-50 dark:hover:text-neutral-100"
          :disabled="store.reloadingFrameworks"
          title="Re-read your own definitions in etc\frameworks.json"
          @click="store.reloadFrameworks()"
        >
          {{ store.reloadingFrameworks ? 'Reloading…' : 'Reload frameworks.json' }}
        </button>
        <div class="flex gap-2">
          <button
            type="button"
            class="rounded-full px-4 py-2 text-sm font-semibold text-neutral-600 dark:text-neutral-300"
            @click="store.closeFrameworkModal()"
          >
            Cancel
          </button>
          <button
            type="button"
            class="glass-accent rounded-full px-4 py-2 text-sm font-semibold transition disabled:opacity-50"
            :disabled="unchanged || choicePending || store.settingFramework"
            @click="save"
          >
            {{ store.settingFramework ? 'Applying…' : 'Save' }}
          </button>
        </div>
      </div>
    </div>
  </div>
</template>
