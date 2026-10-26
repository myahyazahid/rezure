<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref, watch } from 'vue'
import { RouterLink } from 'vue-router'
import type { DevServerIssue, DevServerRow, PortMode } from '@/types/devServer'
import type { PortHolder } from '@/types/service'
import { useDevServersStore } from '@/stores/devServers'
import { useProjectsStore } from '@/stores/projects'
import { useServicesStore } from '@/stores/services'
import BasePill from '@/components/common/BasePill.vue'
import LeafLoader from '@/components/common/LeafLoader.vue'
import ServiceLogPanel from '@/components/services/ServiceLogPanel.vue'

/**
 * Every `proxy` project's dev server: the port nginx sends its domain to,
 * what's run on it, and starting, stopping and installing it. The port, the
 * command and whether it can start are all decided in Rust
 * (`services::dev_servers`) — a field here is only a request to change one.
 */
const store = useDevServersStore()
const projects = useProjectsStore()
const services = useServicesStore()

/** Set when a start failed because something else holds the row's port —
 *  the one failure with an obvious next step, same as a service card's. */
const blocked = ref<{ projectId: string; holder: PortHolder } | null>(null)
const freeing = ref(false)

async function start(row: DevServerRow) {
  blocked.value = null
  const started = await store.start(row.projectId)
  if (started) return
  const holder = await services.portHolder(row.port).catch(() => null)
  if (holder) blocked.value = { projectId: row.projectId, holder }
}

/** Kills whatever holds the port, then starts the dev server — two steps,
 *  so a failure to free it is what gets reported, not a confusing start
 *  error. */
async function freeAndStart(row: DevServerRow) {
  if (!blocked.value) return
  freeing.value = true
  try {
    const still = await services.freePort(blocked.value.holder.port)
    if (still) {
      blocked.value = { projectId: row.projectId, holder: still }
      return
    }
    blocked.value = null
    await start(row)
  } catch (e) {
    store.error = typeof e === 'string' ? e : 'Could not free the port.'
  } finally {
    freeing.value = false
  }
}

const PORT_MODES: { value: PortMode; label: string }[] = [
  { value: 'vite', label: '--port + --strictPort' },
  { value: 'flag', label: '--port' },
  { value: 'env', label: 'PORT variable only' },
  { value: 'fixed', label: "The app's own port" },
]

function portModeLabel(mode: PortMode) {
  return PORT_MODES.find((m) => m.value === mode)?.label ?? mode
}

/** What each row's fields currently hold, before Save. */
interface Draft {
  port: number
  portMode: PortMode | null
  script: string | null
  autoStart: boolean
}
const drafts = reactive<Record<string, Draft>>({})

function draftFrom(row: DevServerRow): Draft {
  return {
    port: row.port,
    portMode: row.portModeSetting,
    script: row.scriptSetting,
    autoStart: row.autoStart,
  }
}

// A fresh read replaces every draft that wasn't being edited, so a saved
// change (or one made elsewhere) shows as it now is.
watch(
  () => store.rows,
  (rows) => {
    for (const row of rows) {
      const draft = drafts[row.projectId]
      if (!draft || !isDirty(row)) drafts[row.projectId] = draftFrom(row)
    }
  },
  { immediate: true },
)

function isDirty(row: DevServerRow) {
  const draft = drafts[row.projectId]
  if (!draft) return false
  const saved = draftFrom(row)
  return (
    draft.port !== saved.port ||
    draft.portMode !== saved.portMode ||
    (draft.script || null) !== saved.script ||
    draft.autoStart !== saved.autoStart
  )
}

function isRunning(row: DevServerRow) {
  return row.status !== 'stopped'
}

function save(row: DevServerRow) {
  const draft = drafts[row.projectId]
  if (!draft) return
  store.save(row.projectId, {
    port: Number(draft.port),
    portMode: draft.portMode,
    script: draft.script || null,
    autoStart: draft.autoStart,
  })
}

function revert(row: DevServerRow) {
  drafts[row.projectId] = draftFrom(row)
}

function toggleLog(row: DevServerRow) {
  store.logFor = store.logFor === row.projectId ? null : row.projectId
}

/** A sentence for the issue, worded for this dialog. */
function issueText(issue: DevServerIssue): string {
  switch (issue.kind) {
    case 'noPackageJson':
      return 'No package.json in this folder — there is nothing to run.'
    case 'nodeMissing':
      return issue.version
        ? `Set to Node.js ${issue.version}, which isn't installed.`
        : "Node.js isn't installed."
    case 'packageManagerMissing':
      return `This project uses ${issue.tool}, which isn't installed. Run "corepack enable" in a terminal, or install ${issue.tool}.`
    case 'scriptMissing':
      return issue.script
        ? `package.json has no "${issue.script}" script — pick another below.`
        : 'package.json has no dev or start script — pick the one that runs the server.'
    case 'needsInstall':
      return "Its dependencies aren't installed yet."
    case 'alreadyRunning': {
      const where = issue.servers
        .map((s) => `${s.name}, pid ${s.pid}, on ${s.ports.map((p) => `:${p}`).join(' ')}`)
        .join('; ')
      return `It's already running outside Rezure (${where}) — probably an "npm run dev" in a terminal. A second copy can't start beside it.`
    }
  }
}

const STATUS_LABEL: Record<string, string> = {
  running: 'Running',
  starting: 'Starting…',
  stopping: 'Stopping…',
  stopped: 'Stopped',
}

function statusClass(row: DevServerRow) {
  switch (store.statusOf(row.projectId) === 'starting' ? 'starting' : row.status) {
    case 'running':
      return 'bg-emerald-500'
    case 'starting':
    case 'stopping':
      return 'bg-amber-400 animate-pulse'
    default:
      return 'bg-neutral-300 dark:bg-neutral-600'
  }
}

const anyRunning = computed(() => store.rows.some((row) => row.status === 'running'))
const busy = computed(() => Object.keys(store.pending).length > 0 || store.stoppingAll)

function close() {
  store.closeModal()
  // The list's framework column shows each port.
  projects.fetchAll()
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') close()
}

onMounted(() => window.addEventListener('keydown', onKeydown))
onUnmounted(() => window.removeEventListener('keydown', onKeydown))

const FIELD_CLASS =
  'glass-inset w-full rounded-lg px-2.5 py-1.5 text-sm text-neutral-800 outline-none focus:ring-2 focus:ring-accent-500/40 disabled:opacity-60 dark:text-neutral-100'
const ROW_BUTTON_CLASS =
  'shrink-0 rounded-full px-3.5 py-1.5 text-xs font-semibold transition disabled:cursor-not-allowed disabled:opacity-50'
</script>

<template>
  <div
    class="glass-scrim fixed inset-0 z-50 flex items-center justify-center p-4"
    @click.self="close"
  >
    <div class="glass-strong flex max-h-[88vh] w-full max-w-3xl flex-col rounded-3xl p-6">
      <div class="flex items-start justify-between gap-4">
        <div>
          <h2 class="text-xl font-bold tracking-tight">Dev servers</h2>
          <p class="mt-1 text-sm text-neutral-500">
            JavaScript and Node projects are served by their own dev server. Nginx passes each
            domain to its port; Rezure starts the server when you click Open, and stops every one
            when it closes.
          </p>
        </div>
        <button
          type="button"
          :class="[ROW_BUTTON_CLASS, 'glass-btn text-neutral-700 dark:text-neutral-200']"
          :disabled="!anyRunning || busy"
          @click="store.stopAll()"
        >
          {{ store.stoppingAll ? 'Stopping…' : 'Stop all' }}
        </button>
      </div>

      <div v-if="store.loading" class="my-10 flex justify-center">
        <LeafLoader :size="40" label="Loading dev servers…" />
      </div>

      <p v-else-if="store.rows.length === 0" class="my-8 text-center text-sm text-neutral-500">
        No project here runs a dev server. Projects detected as Vue, React, Svelte, Angular, Astro,
        Next.js, Nuxt or Node show up here — or pick one of those frameworks from a project's
        framework badge.
      </p>

      <ul v-else class="-mx-1 mt-4 flex flex-1 flex-col gap-3 overflow-y-auto px-1">
        <li
          v-for="row in store.rows"
          :key="row.projectId"
          class="glass-inset overflow-hidden rounded-2xl"
        >
          <div class="p-3.5">
            <div class="flex items-center gap-2">
              <span
                class="h-2 w-2 shrink-0 rounded-full"
                :class="statusClass(row)"
                aria-hidden="true"
              ></span>
              <span class="truncate font-semibold text-neutral-900 dark:text-neutral-100">{{
                row.name
              }}</span>
              <span class="truncate font-mono text-xs text-accent-600 dark:text-accent-400">{{
                row.domain
              }}</span>
              <BasePill class="shrink-0">{{ row.framework }}</BasePill>
              <span class="ml-auto shrink-0 text-xs text-neutral-500">
                {{ store.pending[row.projectId] ?? STATUS_LABEL[row.status] }}
              </span>
            </div>

            <div v-if="drafts[row.projectId]" class="mt-3 grid grid-cols-2 gap-2.5 sm:grid-cols-4">
              <label class="text-xs text-neutral-500">
                Port
                <input
                  v-model.number="drafts[row.projectId]!.port"
                  type="number"
                  min="1024"
                  max="65535"
                  :class="[FIELD_CLASS, 'mt-1 font-mono']"
                  :disabled="isRunning(row)"
                />
              </label>
              <label class="text-xs text-neutral-500">
                Tell it the port with
                <select
                  v-model="drafts[row.projectId]!.portMode"
                  :class="[FIELD_CLASS, 'mt-1']"
                  :disabled="isRunning(row)"
                >
                  <option :value="null">Default ({{ portModeLabel(row.defaultPortMode) }})</option>
                  <option v-for="mode in PORT_MODES" :key="mode.value" :value="mode.value">
                    {{ mode.label }}
                  </option>
                </select>
              </label>
              <label class="text-xs text-neutral-500">
                Script
                <select
                  v-model="drafts[row.projectId]!.script"
                  :class="[FIELD_CLASS, 'mt-1 font-mono']"
                  :disabled="isRunning(row)"
                >
                  <option :value="null">
                    Default{{ row.scriptSetting === null && row.script ? ` (${row.script})` : '' }}
                  </option>
                  <option v-for="script in row.scripts" :key="script" :value="script">
                    {{ script }}
                  </option>
                </select>
              </label>
              <label
                class="flex items-end gap-2 pb-2 text-xs text-neutral-600 dark:text-neutral-300"
                title="Clicking Open on the project starts its dev server first"
              >
                <input
                  v-model="drafts[row.projectId]!.autoStart"
                  type="checkbox"
                  class="h-4 w-4 accent-accent-500"
                />
                Open starts it
              </label>
            </div>

            <p
              v-if="row.command"
              class="mt-2 truncate font-mono text-[11px] text-neutral-500"
              :title="row.command"
            >
              {{ row.packageManager === 'npm' ? '' : `${row.packageManager} · ` }}{{ row.command }}
            </p>
            <p v-if="isRunning(row)" class="mt-1 text-[11px] text-neutral-400">
              Stop it to change its port or script.
            </p>

            <div
              v-if="row.issue && !isRunning(row)"
              class="mt-2.5 flex items-center justify-between gap-3 rounded-xl bg-amber-100/50 px-3 py-2 text-xs text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
            >
              <span>{{ issueText(row.issue) }}</span>
              <RouterLink
                v-if="row.issue.kind === 'nodeMissing'"
                to="/switch"
                class="glass-btn shrink-0 rounded-full px-3 py-1 font-semibold"
                @click="close"
              >
                Open Switch
              </RouterLink>
              <button
                v-else-if="row.issue.kind === 'needsInstall'"
                type="button"
                :class="[ROW_BUTTON_CLASS, 'glass-accent']"
                :disabled="Boolean(store.pending[row.projectId])"
                @click="store.install(row.projectId)"
              >
                {{
                  store.pending[row.projectId] === 'Installing…'
                    ? 'Installing…'
                    : `${row.packageManager} install`
                }}
              </button>
              <!-- Stops only that copy's node process — the terminal it was
                   typed in stays open — then starts Rezure's own. -->
              <button
                v-else-if="row.issue.kind === 'alreadyRunning'"
                type="button"
                :class="[ROW_BUTTON_CLASS, 'glass-accent']"
                :disabled="Boolean(store.pending[row.projectId])"
                @click="store.replaceOutside(row.projectId)"
              >
                {{ store.pending[row.projectId] ?? 'Stop it and start here' }}
              </button>
            </div>

            <div class="mt-3 flex items-center justify-end gap-2">
              <button
                type="button"
                :class="[ROW_BUTTON_CLASS, 'glass-ghost text-neutral-600 dark:text-neutral-300']"
                @click="toggleLog(row)"
              >
                {{ store.logFor === row.projectId ? 'Hide log' : 'Log' }}
              </button>
              <template v-if="isDirty(row)">
                <button
                  type="button"
                  :class="[ROW_BUTTON_CLASS, 'glass-ghost text-neutral-600 dark:text-neutral-300']"
                  :disabled="Boolean(store.pending[row.projectId])"
                  @click="revert(row)"
                >
                  Revert
                </button>
                <button
                  type="button"
                  :class="[ROW_BUTTON_CLASS, 'glass-accent']"
                  :disabled="Boolean(store.pending[row.projectId])"
                  @click="save(row)"
                >
                  {{ store.pending[row.projectId] === 'Saving…' ? 'Saving…' : 'Save' }}
                </button>
              </template>
              <button
                v-else-if="row.status === 'stopped'"
                type="button"
                :class="[ROW_BUTTON_CLASS, 'glass-accent']"
                :disabled="Boolean(store.pending[row.projectId]) || row.issue !== null"
                :title="row.issue ? issueText(row.issue) : `Start ${row.command ?? ''}`"
                @click="start(row)"
              >
                {{ store.pending[row.projectId] === 'Starting…' ? 'Starting…' : 'Start' }}
              </button>
              <button
                v-else
                type="button"
                :class="[ROW_BUTTON_CLASS, 'glass-btn text-neutral-700 dark:text-neutral-200']"
                :disabled="Boolean(store.pending[row.projectId]) || row.status === 'stopping'"
                @click="store.stop(row.projectId)"
              >
                {{ store.pending[row.projectId] === 'Stopping…' ? 'Stopping…' : 'Stop' }}
              </button>
            </div>
          </div>

          <ServiceLogPanel v-if="store.logFor === row.projectId" :service-id="row.serviceId" />
        </li>
      </ul>

      <p v-if="store.error" class="mt-3 text-sm text-red-600 dark:text-red-400">
        {{ store.error }}
      </p>
      <div
        v-if="blocked"
        class="mt-2 flex items-center justify-between gap-3 rounded-xl bg-amber-100/50 px-3 py-2 text-xs text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
      >
        <span>{{ blocked.holder.description }}</span>
        <button
          v-if="blocked.holder.kind !== 'system'"
          type="button"
          :class="[ROW_BUTTON_CLASS, 'glass-accent']"
          :disabled="freeing"
          @click="freeAndStart(store.rows.find((row) => row.projectId === blocked?.projectId)!)"
        >
          {{ freeing ? 'Freeing…' : `Free port ${blocked.holder.port} and start` }}
        </button>
      </div>

      <div class="mt-5 flex items-center justify-between gap-3">
        <p class="text-xs text-neutral-400">
          New ports come from 4400–4499. Settings live in etc\dev-servers.json.
        </p>
        <button
          type="button"
          class="glass-accent rounded-full px-4 py-2 text-sm font-semibold transition"
          @click="close"
        >
          Done
        </button>
      </div>
    </div>
  </div>
</template>
