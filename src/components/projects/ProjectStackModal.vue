<script setup lang="ts">
import { computed } from 'vue'
import { useRouter } from 'vue-router'
import { useProjectsStore } from '@/stores/projects'
import ModalCloseButton from '@/components/common/ModalCloseButton.vue'
import {
  databaseProblem,
  packageManagerHint,
  runtimeFix,
  runtimeMismatch,
  stackProblems,
  type Runtime,
} from '@/composables/projectStack'
import type { DatabaseNeed, RuntimeRequirement } from '@/types/project'

/**
 * Everything Rezure could read about what a project is built on and needs:
 * frameworks and their versions, PHP, Node.js, the package manager, the
 * database — each against what Rezure has, with the fix beside anything
 * that doesn't fit. The facts are Rust's (`services::project_stack`).
 */
const store = useProjectsStore()
const router = useRouter()

const project = computed(() => store.projects.find((p) => p.id === store.stackModalFor) ?? null)
const stack = computed(() => project.value?.stackInfo ?? null)
const problems = computed(() => (stack.value ? stackProblems(stack.value) : []))

interface RuntimeRow {
  runtime: Runtime
  requirement: RuntimeRequirement
  pinned: string | null
  busy: boolean
  error: string | null
}

const runtimes = computed<RuntimeRow[]>(() => {
  const rows: RuntimeRow[] = []
  if (!project.value || !stack.value) return rows
  if (stack.value.php) {
    rows.push({
      runtime: 'PHP',
      requirement: stack.value.php,
      pinned: project.value.phpVersion,
      busy: store.settingPhpVersion,
      error: store.phpVersionError,
    })
  }
  if (stack.value.node) {
    rows.push({
      runtime: 'Node.js',
      requirement: stack.value.node,
      pinned: project.value.nodeVersion,
      busy: store.settingNodeVersion,
      error: store.nodeVersionError,
    })
  }
  return rows
})

function close() {
  store.closeStackModal()
}

function pin(row: RuntimeRow, version: string) {
  if (!project.value) return
  if (row.runtime === 'PHP') store.setPhpVersion(project.value.id, version)
  else store.setNodeVersion(project.value.id, version)
}

/** The full version picker, for choosing something other than the suggestion. */
function openPicker(row: RuntimeRow) {
  if (!project.value) return
  const id = project.value.id
  close()
  if (row.runtime === 'PHP') store.openPhpVersionModal(id)
  else store.openNodeVersionModal(id)
}

function go(path: string) {
  close()
  router.push(path)
}

function checkExtensions() {
  if (!project.value) return
  const id = project.value.id
  close()
  store.runDoctor(id)
}

function statusOf(fits: boolean | null): { label: string; class: string } {
  if (fits === true) {
    return { label: 'Fits', class: 'bg-emerald-500/15 text-emerald-700 dark:text-emerald-300' }
  }
  if (fits === false) {
    return { label: "Doesn't fit", class: 'bg-amber-500/15 text-amber-700 dark:text-amber-300' }
  }
  return { label: 'Unknown', class: 'bg-neutral-500/10 text-neutral-500' }
}

/** Where a database stands in Rezure, as one line. */
function databaseState(db: DatabaseNeed): string {
  if (db.engine === 'sqlite') return 'A file inside the project - nothing to run.'
  if (!db.supported || !db.provider) return "Rezure doesn't provide it."
  if (!db.provider.installed) return 'Not installed in Rezure.'
  const name = [db.provider.name, db.provider.version].filter(Boolean).join(' ')
  return `Served by Rezure's ${name} - ${db.provider.running ? 'running' : 'stopped'}.`
}

const SECTION_TITLE = 'text-[11px] font-semibold tracking-wide text-neutral-500 uppercase'
const CARD = 'glass-inset rounded-xl px-4 py-3'
const SMALL_BUTTON =
  'glass-btn rounded-full px-3 py-1 text-xs font-semibold text-neutral-700 transition disabled:opacity-50 dark:text-neutral-200'
</script>

<template>
  <div
    v-if="store.stackModalFor && project && stack"
    class="glass-scrim fixed inset-0 z-50 flex items-center justify-center p-4"
  >
    <div class="glass-strong relative flex max-h-[88vh] w-full max-w-2xl flex-col rounded-3xl p-6">
      <ModalCloseButton @click="close" />
      <h2 class="pr-8 text-xl font-bold tracking-tight">
        Stack &amp; requirements
        <span class="font-normal text-neutral-400">· {{ project.name }}</span>
      </h2>
      <p class="mt-1 text-sm text-neutral-500">
        Read from the project's own files - composer.json, package.json, .env and the like - and
        checked against what Rezure has installed. Nothing is run.
      </p>

      <div class="-mx-1 mt-4 flex-1 space-y-5 overflow-y-auto px-1">
        <div
          v-if="problems.length"
          class="rounded-xl bg-amber-100/60 px-4 py-3 text-sm text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
        >
          <p class="font-semibold">
            {{
              problems.length === 1
                ? 'One thing needs fixing'
                : `${problems.length} things need fixing`
            }}
            before this project runs as it should
          </p>
          <ul class="mt-1 list-disc space-y-0.5 pl-5">
            <li v-for="problem in problems" :key="problem">{{ problem }}</li>
          </ul>
        </div>

        <!-- Frameworks -->
        <section v-if="stack.frameworks.length">
          <p :class="SECTION_TITLE">Built with</p>
          <div class="mt-2 flex flex-wrap gap-1.5">
            <span
              v-for="framework in stack.frameworks"
              :key="framework.name"
              class="glass-inset rounded-full px-3 py-1 text-xs"
              :title="
                framework.installed ? 'Installed version' : 'Declared range - not installed yet'
              "
            >
              <span class="font-semibold text-neutral-800 dark:text-neutral-100">{{
                framework.name
              }}</span>
              <span v-if="framework.version" class="ml-1 font-mono text-neutral-500">{{
                framework.version
              }}</span>
              <span v-if="!framework.installed" class="ml-1 text-amber-600 dark:text-amber-400"
                >· not installed</span
              >
            </span>
          </div>
          <p
            v-if="stack.frameworks.some((f) => !f.installed)"
            class="mt-1.5 text-xs text-neutral-500"
          >
            "Not installed" means the dependencies haven't been installed yet - run
            <span class="font-mono">composer install</span> or your package manager's install in the
            project's terminal.
          </p>
        </section>

        <!-- PHP and Node.js -->
        <section v-if="runtimes.length">
          <p :class="SECTION_TITLE">Runtimes</p>
          <div class="mt-2 space-y-2">
            <div v-for="row in runtimes" :key="row.runtime" :class="CARD">
              <div class="flex items-center justify-between gap-3">
                <p class="text-sm font-semibold text-neutral-900 dark:text-neutral-100">
                  {{ row.runtime }}
                  <span
                    class="ml-1.5 rounded-full px-2 py-0.5 text-[10px] font-semibold"
                    :class="statusOf(row.requirement.fits).class"
                    >{{ statusOf(row.requirement.fits).label }}</span
                  >
                </p>
                <button type="button" :class="SMALL_BUTTON" @click="openPicker(row)">
                  Change version
                </button>
              </div>
              <p class="mt-1 text-xs text-neutral-500">
                <template v-if="row.requirement.serving">
                  Runs on
                  <span class="font-mono text-neutral-700 dark:text-neutral-200"
                    >{{ row.runtime }} {{ row.requirement.serving }}</span
                  >
                  {{ row.pinned === row.requirement.serving ? '(pinned)' : '(global default)' }}
                </template>
                <template v-else>
                  No {{ row.runtime }} installed yet - install one on the Switch page.
                </template>
              </p>

              <ul class="mt-2 space-y-1 text-xs">
                <li
                  v-for="stated in row.requirement.stated"
                  :key="stated.source"
                  class="flex items-baseline gap-2"
                >
                  <span
                    class="w-3 shrink-0 text-center font-bold"
                    :class="
                      stated.fits === false
                        ? 'text-amber-600 dark:text-amber-400'
                        : stated.fits
                          ? 'text-emerald-600 dark:text-emerald-400'
                          : 'text-neutral-400'
                    "
                    >{{ stated.fits === false ? '!' : stated.fits ? '✓' : '?' }}</span
                  >
                  <span class="font-mono text-neutral-800 dark:text-neutral-100">{{
                    stated.constraint
                  }}</span>
                  <span class="text-neutral-500">· {{ stated.source }}</span>
                </li>
              </ul>
              <p v-if="!row.requirement.readable" class="mt-1.5 text-xs text-neutral-500">
                Rezure can't read this constraint - check it against your versions yourself.
              </p>

              <div
                v-if="runtimeMismatch(row.requirement)"
                class="mt-3 flex flex-wrap items-center justify-between gap-2 rounded-lg bg-amber-100/50 px-3 py-2 text-xs text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
              >
                <span class="min-w-0 flex-1">{{ runtimeFix(row.runtime, row.requirement) }}</span>
                <button
                  v-if="row.requirement.suggested"
                  type="button"
                  class="glass-accent shrink-0 rounded-full px-3 py-1 text-xs font-semibold transition disabled:opacity-50"
                  :disabled="row.busy"
                  @click="pin(row, row.requirement.suggested)"
                >
                  Pin to {{ row.runtime }} {{ row.requirement.suggested }}
                </button>
                <button
                  v-else
                  type="button"
                  class="glass-accent shrink-0 rounded-full px-3 py-1 text-xs font-semibold transition"
                  @click="go('/switch')"
                >
                  Open Switch
                </button>
              </div>
              <p v-if="row.error" class="mt-2 text-xs text-red-600 dark:text-red-400">
                {{ row.error }}
              </p>
            </div>
          </div>
        </section>

        <!-- Package manager -->
        <section v-if="stack.packageManager">
          <p :class="SECTION_TITLE">Package manager</p>
          <div :class="[CARD, 'mt-2']">
            <p class="text-sm font-semibold text-neutral-900 dark:text-neutral-100">
              {{ stack.packageManager.name }}
              <span
                v-if="stack.packageManager.version"
                class="font-mono font-normal text-neutral-500"
                >{{ stack.packageManager.version }}</span
              >
              <span
                v-if="stack.packageManager.fits !== null"
                class="ml-1.5 rounded-full px-2 py-0.5 text-[10px] font-semibold"
                :class="statusOf(stack.packageManager.fits).class"
                >{{ statusOf(stack.packageManager.fits).label }}</span
              >
            </p>
            <p class="mt-1 text-xs text-neutral-500">
              From {{ stack.packageManager.source }}.
              <template v-if="stack.packageManager.constraint">
                Asks for <span class="font-mono">{{ stack.packageManager.constraint }}</span
                ><template v-if="stack.packageManager.current">
                  - npm {{ stack.packageManager.current }} comes with the Node.js serving
                  it</template
                >.
              </template>
              {{ packageManagerHint(stack.packageManager.name) }}
            </p>
          </div>
        </section>

        <!-- Database -->
        <section v-if="stack.databases.length">
          <p :class="SECTION_TITLE">Database</p>
          <div class="mt-2 space-y-2">
            <div v-for="db in stack.databases" :key="db.engine" :class="CARD">
              <div class="flex items-center justify-between gap-3">
                <p class="text-sm font-semibold text-neutral-900 dark:text-neutral-100">
                  {{ db.label }}
                  <span v-if="db.minimum" class="font-normal text-neutral-500"
                    >{{ db.minimum }} or newer</span
                  >
                </p>
                <button
                  v-if="db.provider && !db.provider.installed"
                  type="button"
                  :class="SMALL_BUTTON"
                  @click="go('/switch')"
                >
                  Install on Switch
                </button>
                <button
                  v-else-if="db.provider && !db.provider.running"
                  type="button"
                  :class="SMALL_BUTTON"
                  @click="go('/')"
                >
                  Open Services
                </button>
              </div>
              <p class="mt-1 text-xs text-neutral-500">
                From {{ db.source }}. {{ databaseState(db) }}
              </p>
              <p
                v-if="databaseProblem(db)"
                class="mt-1.5 text-xs text-amber-700 dark:text-amber-300"
              >
                {{ databaseProblem(db) }}
              </p>
            </div>
          </div>
          <p class="mt-1.5 text-xs text-neutral-400">
            Most frameworks don't state a database version, so only the engine is checked.
          </p>
        </section>
      </div>

      <div class="mt-5 flex items-center justify-between gap-2">
        <button
          v-if="stack.php"
          type="button"
          :class="SMALL_BUTTON"
          title="Checks composer.json's ext-* requirements against the PHP serving this project"
          @click="checkExtensions"
        >
          Check PHP extensions
        </button>
        <span v-else></span>
        <button
          type="button"
          class="glass-accent rounded-full px-4 py-2 text-sm font-semibold transition"
          @click="close"
        >
          Close
        </button>
      </div>
    </div>
  </div>
</template>
