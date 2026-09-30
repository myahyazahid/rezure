<script setup lang="ts">
import { computed, watch } from 'vue'
import { useProjectsStore } from '@/stores/projects'
import { usePhpStore } from '@/stores/php'

const store = useProjectsStore()
const phpStore = usePhpStore()

const project = computed(
  () => store.projects.find((p) => p.id === store.phpVersionModalFor) ?? null,
)

// The picker needs the installed list, which the Projects page never
// otherwise fetches — only the Switch page does.
watch(
  () => store.phpVersionModalFor,
  (id) => {
    if (id && phpStore.versions.length === 0) phpStore.fetchAll()
  },
)

function choose(version: string | null) {
  if (!project.value) return
  store.setPhpVersion(project.value.id, version)
}
</script>

<template>
  <div
    v-if="store.phpVersionModalFor"
    class="glass-scrim fixed inset-0 z-50 flex items-center justify-center p-4"
    @click.self="store.closePhpVersionModal()"
  >
    <div class="glass-strong w-full max-w-lg rounded-2xl p-6">
      <h2 class="text-lg font-bold text-neutral-900 dark:text-neutral-100">
        PHP version
        <span v-if="project" class="font-normal text-neutral-400">· {{ project.name }}</span>
      </h2>
      <p class="mt-1 text-sm text-neutral-500">
        Pin this project to a specific PHP version so it keeps running on it even if you switch the
        global default on the Switch page. Pinning a version other projects aren't on gives this
        project its own PHP process, so different projects can run on different versions at the same
        time. A terminal opened from this project's card uses the same version for
        <span class="font-mono">php</span>/<span class="font-mono">composer</span> — a terminal
        already open keeps whatever it started with.
      </p>

      <div class="mt-5 space-y-1.5">
        <button
          type="button"
          class="flex w-full items-center justify-between rounded-xl px-3.5 py-2.5 text-left text-sm transition"
          :class="
            !project?.phpVersion
              ? 'glass-selected'
              : 'glass-inset hover:bg-white/70 dark:hover:bg-white/8'
          "
          :disabled="store.settingPhpVersion"
          @click="choose(null)"
        >
          <span>
            <span class="font-semibold text-neutral-900 dark:text-neutral-100">Default</span>
            <span class="ml-1.5 text-neutral-400">· follows the global active version</span>
          </span>
          <span
            v-if="!project?.phpVersion"
            class="text-xs font-semibold text-red-600 dark:text-red-400"
            >Current</span
          >
        </button>

        <button
          v-for="version in phpStore.versions"
          :key="version.id"
          type="button"
          class="flex w-full items-center justify-between rounded-xl px-3.5 py-2.5 text-left text-sm transition"
          :class="
            project?.phpVersion === version.version
              ? 'glass-selected'
              : 'glass-inset hover:bg-white/70 dark:hover:bg-white/8'
          "
          :disabled="store.settingPhpVersion"
          @click="choose(version.version)"
        >
          <span>
            <span class="font-semibold text-neutral-900 dark:text-neutral-100"
              >PHP {{ version.version }}</span
            >
            <span v-if="version.active" class="ml-1.5 text-neutral-400"
              >· currently the global default</span
            >
          </span>
          <span
            v-if="project?.phpVersion === version.version"
            class="text-xs font-semibold text-red-600 dark:text-red-400"
            >Current</span
          >
        </button>

        <p v-if="phpStore.versions.length === 0" class="px-1 py-2 text-sm text-neutral-400">
          No PHP versions installed yet — install one from the Switch page first.
        </p>
      </div>

      <p
        v-if="store.phpVersionError"
        class="mt-4 rounded-xl bg-amber-100/50 px-3 py-2 text-sm text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
      >
        {{ store.phpVersionError }}
      </p>

      <div class="mt-6 flex items-center justify-end gap-2">
        <button
          type="button"
          class="glass-accent rounded-full px-4 py-2 text-sm font-semibold transition"
          @click="store.closePhpVersionModal()"
        >
          Close
        </button>
      </div>
    </div>
  </div>
</template>
