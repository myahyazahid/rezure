<script setup lang="ts">
import { computed, ref } from 'vue'
import type { DevServerRow, StartFailure } from '@/types/devServer'
import { useDevServersStore } from '@/stores/devServers'
import { useProjectsStore } from '@/stores/projects'

/**
 * Why a dev server didn't come up, laid out to be acted on: the cause, the
 * recommended fix as a button, and the output that explains it folded away
 * below. Everything here is decided in Rust (`dev_server_failure`) — this
 * only shows it and runs the fix the user picks.
 */
const props = defineProps<{
  row: DevServerRow
  failure: StartFailure
  /** What trying again does where the card is shown — the Projects page
   *  opens the site, the Dev servers dialog only starts the server. */
  retryLabel: 'open' | 'start'
  /** Inside the Dev servers dialog: the row already names the project and
   *  holds the port field, so the card drops both. */
  compact?: boolean
}>()

const emit = defineEmits<{
  /** Start it again (and open it, on the Projects page). */
  retry: []
  dismiss: []
  openDevServers: []
}>()

const devServers = useDevServersStore()
const projects = useProjectsStore()

const pending = computed(() => devServers.pending[props.row.projectId] ?? null)
const fix = computed(() => props.failure.fix)
/** Folded when the cause is known — the output is then only the evidence.
 *  Open when it isn't, since the output is all there is to go on. */
const showOutput = ref(props.failure.cause === null)
const copied = ref(false)
const actionError = ref<string | null>(null)

const retryText = computed(() => (props.retryLabel === 'open' ? 'open' : 'start'))

function capitalize(text: string) {
  return text.charAt(0).toUpperCase() + text.slice(1)
}

const headline = computed(() =>
  props.compact ? 'The last start failed' : `${props.row.name}'s dev server didn't start`,
)
const cause = computed(() => props.failure.cause ?? capitalize(props.failure.what))

/** A sentence on why the recommended step is the right one. */
const fixExplanation = computed(() => {
  const f = fix.value
  if (!f) {
    return "Rezure doesn't recognize this failure. What the dev server printed is below."
  }
  switch (f.kind) {
    case 'install':
      return f.package
        ? `package.json lists ${f.package}, so installing the project's dependencies brings it.`
        : "The project's dependencies aren't fully installed."
    case 'addDependency':
      return `${f.package} isn't listed in package.json, so Install won't bring it. Add it from the project folder:`
    case 'stopOther':
      return 'Another copy is already running, probably from a terminal or an editor.'
    case 'usePort':
      return `It ignored the port Rezure gave it and came up on ${f.found
        .map((port) => `:${port}`)
        .join(
          ', ',
        )} instead of :${f.expected}. Set that port in Dev servers, or make the app use :${f.expected}.`
  }
  return ''
})

/** Runs a fix through the Dev servers store, then tries again if it worked. */
async function runFix(action: () => Promise<boolean>) {
  actionError.value = null
  const ok = await action()
  if (ok) emit('retry')
  else actionError.value = devServers.error
}

function installAndRetry() {
  runFix(() => devServers.install(props.row.projectId))
}

function stopOtherAndRetry() {
  // Starts Rezure's own copy too once the other one is gone.
  runFix(() => devServers.replaceOutside(props.row.projectId))
}

async function copyCommand(command: string) {
  await navigator.clipboard.writeText(command)
  copied.value = true
  setTimeout(() => (copied.value = false), 1500)
}

const PRIMARY =
  'glass-accent rounded-full px-3.5 py-1.5 text-xs font-semibold transition disabled:opacity-50'
const SECONDARY =
  'glass-btn rounded-full px-3.5 py-1.5 text-xs font-semibold text-neutral-700 transition disabled:opacity-50 dark:text-neutral-200'
</script>

<template>
  <div
    class="rounded-2xl border border-red-300/60 bg-red-50/70 text-sm dark:border-red-500/25 dark:bg-red-500/10"
    :class="compact ? 'p-3' : 'p-4'"
    role="alert"
  >
    <div class="flex items-start gap-3">
      <span
        class="mt-0.5 flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-red-500/15 text-red-600 dark:text-red-400"
        aria-hidden="true"
      >
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2.5"
          class="h-3.5 w-3.5"
        >
          <path stroke-linecap="round" d="M12 7v6M12 17h.01" />
        </svg>
      </span>

      <div class="min-w-0 flex-1">
        <p class="font-semibold text-red-700 dark:text-red-300">{{ headline }}</p>
        <p class="mt-0.5 break-words text-neutral-800 dark:text-neutral-100">{{ cause }}</p>

        <!-- The recommended step, with the button that takes it. -->
        <div class="glass-inset mt-3 rounded-xl p-3">
          <p class="text-[11px] font-semibold tracking-wide text-neutral-500 uppercase">
            {{ fix ? 'Recommended fix' : 'What to try' }}
          </p>
          <p class="mt-1 text-xs text-neutral-700 dark:text-neutral-200">{{ fixExplanation }}</p>

          <!-- Shown to copy, never run: the package name comes from the dev
               server's own output. -->
          <div
            v-if="fix?.kind === 'addDependency'"
            class="mt-2 flex items-center gap-2 rounded-lg bg-neutral-900/90 py-1.5 pr-1.5 pl-3 dark:bg-black/40"
          >
            <code class="min-w-0 flex-1 truncate font-mono text-xs text-neutral-100">{{
              fix.command
            }}</code>
            <button
              type="button"
              class="shrink-0 rounded-md px-2 py-1 text-[11px] font-semibold text-neutral-200 transition hover:bg-white/10"
              @click="copyCommand(fix.command)"
            >
              {{ copied ? 'Copied' : 'Copy' }}
            </button>
          </div>

          <div class="mt-2.5 flex flex-wrap items-center gap-2">
            <button
              v-if="fix?.kind === 'install'"
              type="button"
              :class="PRIMARY"
              :disabled="pending !== null"
              @click="installAndRetry"
            >
              {{ pending ?? `${row.packageManager} install & ${retryText}` }}
            </button>
            <button
              v-else-if="fix?.kind === 'stopOther'"
              type="button"
              :class="PRIMARY"
              :disabled="pending !== null"
              @click="stopOtherAndRetry"
            >
              {{ pending ?? `Stop the other copy & ${retryText}` }}
            </button>
            <template v-else-if="fix?.kind === 'addDependency'">
              <button
                type="button"
                :class="SECONDARY"
                @click="projects.openTerminal(row.projectId)"
              >
                Open terminal here
              </button>
              <button
                type="button"
                :class="PRIMARY"
                :disabled="pending !== null"
                @click="emit('retry')"
              >
                {{ pending ?? 'Try again' }}
              </button>
            </template>
            <button
              v-else-if="fix?.kind === 'usePort' && !compact"
              type="button"
              :class="PRIMARY"
              @click="emit('openDevServers')"
            >
              Set the port in Dev servers
            </button>
            <button
              v-else-if="!fix"
              type="button"
              :class="PRIMARY"
              :disabled="pending !== null"
              @click="emit('retry')"
            >
              {{ pending ?? 'Try again' }}
            </button>

            <button
              v-if="!compact && fix?.kind !== 'usePort'"
              type="button"
              :class="SECONDARY"
              @click="emit('openDevServers')"
            >
              Open Dev servers
            </button>
          </div>
          <p v-if="actionError" class="mt-2 text-xs text-red-600 dark:text-red-400">
            {{ actionError }}
          </p>
        </div>

        <!-- The evidence: what the dev server printed, noise taken out. -->
        <div v-if="failure.output.length > 0" class="mt-2.5">
          <button
            type="button"
            class="flex items-center gap-1 text-xs font-semibold text-neutral-600 transition hover:text-neutral-900 dark:text-neutral-300 dark:hover:text-white"
            :aria-expanded="showOutput"
            @click="showOutput = !showOutput"
          >
            <svg
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2.5"
              class="h-3 w-3 transition-transform"
              :class="showOutput ? 'rotate-90' : ''"
              aria-hidden="true"
            >
              <path stroke-linecap="round" stroke-linejoin="round" d="m9 6 6 6-6 6" />
            </svg>
            What it printed
            <span class="font-normal text-neutral-400">· {{ failure.what }}</span>
          </button>
          <pre
            v-if="showOutput"
            class="mt-1.5 max-h-48 overflow-auto rounded-lg bg-neutral-900/90 p-3 font-mono text-[11px] leading-relaxed whitespace-pre-wrap text-neutral-100 dark:bg-black/40"
            >{{ failure.output.join('\n') }}</pre>
        </div>
      </div>

      <button
        v-if="!compact"
        type="button"
        title="Dismiss"
        class="glass-ghost flex h-7 w-7 shrink-0 items-center justify-center rounded-full text-neutral-500 transition"
        @click="emit('dismiss')"
      >
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          class="h-3.5 w-3.5"
        >
          <path stroke-linecap="round" d="M6 6l12 12M18 6 6 18" />
        </svg>
      </button>
    </div>
  </div>
</template>
