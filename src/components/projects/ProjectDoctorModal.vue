<script setup lang="ts">
import { computed } from 'vue'
import { useProjectsStore } from '@/stores/projects'
import { usePhpStore } from '@/stores/php'

const store = useProjectsStore()
const phpStore = usePhpStore()

const project = computed(() => store.projects.find((p) => p.id === store.doctorFor) ?? null)
const result = computed(() => store.diagnosis)
const loading = computed(() => store.doctorFor !== null && !result.value && !store.doctorError)

/** The one number that decides what this modal is saying. */
const missingCount = computed(() => result.value?.missing.length ?? 0)
</script>

<template>
  <div
    v-if="store.doctorFor"
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
    @click.self="store.closeDoctor()"
  >
    <div
      class="w-full max-w-lg rounded-2xl border border-neutral-200 bg-white p-6 shadow-2xl dark:border-neutral-700 dark:bg-neutral-900"
    >
      <h2 class="text-lg font-bold text-neutral-900 dark:text-neutral-100">
        Requirements check
        <span v-if="project" class="font-normal text-neutral-400">· {{ project.name }}</span>
      </h2>
      <p class="mt-1 text-sm text-neutral-500">
        Every <code class="font-mono">ext-*</code> in this project's
        <code class="font-mono">composer.json</code>, checked against the PHP that serves it.
      </p>

      <p v-if="loading" class="mt-5 text-sm text-neutral-500">Asking PHP…</p>

      <p
        v-else-if="store.doctorError"
        class="mt-5 rounded-xl bg-amber-50 px-3 py-2 text-sm text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
      >
        {{ store.doctorError }}
      </p>

      <template v-else-if="result">
        <!-- No composer.json and no ext-* are different findings, and both
             are results rather than failures: most of www is WordPress and
             static folders. -->
        <p v-if="!result.hasComposerJson" class="mt-5 text-sm text-neutral-500">
          No <code class="font-mono">composer.json</code> here, so there's nothing to check.
        </p>
        <p v-else-if="result.extensions.length === 0" class="mt-5 text-sm text-neutral-500">
          This project doesn't require any PHP extension explicitly.
        </p>

        <template v-else>
          <p
            class="mt-5 rounded-xl px-3 py-2 text-sm"
            :class="
              missingCount > 0
                ? 'bg-amber-50 text-amber-900 dark:bg-amber-500/10 dark:text-amber-200'
                : 'bg-emerald-50 text-emerald-800 dark:bg-emerald-500/10 dark:text-emerald-300'
            "
          >
            <template v-if="missingCount > 0">
              PHP {{ result.phpVersion }} is missing <strong>{{ result.missing.join(', ') }}</strong
              >. That's the kind of gap that shows up as a blank 500 with the reason only in
              <code class="font-mono">laravel.log</code>.
            </template>
            <template v-else>
              PHP {{ result.phpVersion }} has everything this project asks for.
            </template>
          </p>

          <ul class="mt-4 flex flex-col gap-1.5">
            <li
              v-for="check in result.extensions"
              :key="check.name"
              class="flex items-center gap-2 text-sm"
            >
              <span
                class="flex h-5 w-5 shrink-0 items-center justify-center rounded-full text-[11px] font-bold"
                :class="
                  check.loaded
                    ? 'bg-emerald-100 text-emerald-700 dark:bg-emerald-500/15 dark:text-emerald-400'
                    : check.devOnly
                      ? 'bg-neutral-200 text-neutral-500 dark:bg-neutral-800 dark:text-neutral-400'
                      : 'bg-amber-100 text-amber-700 dark:bg-amber-500/15 dark:text-amber-300'
                "
              >
                {{ check.loaded ? '✓' : '!' }}
              </span>
              <code class="font-mono text-neutral-800 dark:text-neutral-200">{{ check.name }}</code>
              <span v-if="check.devOnly" class="text-xs text-neutral-400">dev only</span>
            </li>
          </ul>

          <!-- The next step, not just the diagnosis: enabling an extension
               is a line in the settings folder, and that folder is the one
               place an edit survives a restart and a version switch. -->
          <p v-if="missingCount > 0" class="mt-4 text-xs text-neutral-500">
            Add <code class="font-mono">extension={{ result.missing[0] }}</code> to an
            <code class="font-mono">.ini</code> file in your settings folder, then restart PHP. If
            the DLL isn't in the build, it has to be installed first.
          </p>
        </template>
      </template>

      <div class="mt-6 flex items-center justify-end gap-2">
        <button
          v-if="missingCount > 0"
          type="button"
          class="rounded-full border border-neutral-200 px-4 py-2 text-sm font-semibold text-neutral-700 transition hover:bg-neutral-100 dark:border-neutral-700 dark:text-neutral-200 dark:hover:bg-neutral-800"
          @click="phpStore.openConfigDir()"
        >
          Open settings folder
        </button>
        <button
          type="button"
          class="rounded-full bg-red-600 px-4 py-2 text-sm font-semibold text-white transition hover:bg-red-500"
          @click="store.closeDoctor()"
        >
          Close
        </button>
      </div>
    </div>
  </div>
</template>
