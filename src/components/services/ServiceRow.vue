<script setup lang="ts">
import { computed, ref } from 'vue'
import type { PortHolder, ServiceInfo } from '@/types/service'
import { useServicesStore } from '@/stores/services'
import BasePill from '@/components/common/BasePill.vue'
import ServiceSparkline from '@/components/services/ServiceSparkline.vue'
import ServiceLogPanel from '@/components/services/ServiceLogPanel.vue'

const props = defineProps<{ service: ServiceInfo }>()

const store = useServicesStore()
const expanded = ref(false)
const menuOpen = ref(false)
/** Force stop asks before acting — it skips the clean shutdown, which for a
 *  database means crash recovery on the next start. */
const confirmingForceStop = ref(false)

const isRunning = computed(() => props.service.status === 'running')
const isPending = computed(() => store.isPending(props.service.id))
const initial = computed(() => props.service.name.charAt(0).toUpperCase())
const error = ref<string | null>(null)

function toggleExpanded() {
  expanded.value = !expanded.value
}

function errorMessage(e: unknown): string {
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  return 'Something went wrong.'
}

/** Set when a start failed because something else owns the port. Drives the
 *  "take it back" offer, which is the only way out of that failure short of
 *  hunting the process down by hand. */
const blocker = ref<PortHolder | null>(null)
const freeing = ref(false)

async function onPrimaryAction() {
  error.value = null
  blocker.value = null
  try {
    await (isRunning.value ? store.stop(props.service.id) : store.start(props.service.id))
  } catch (e) {
    error.value = errorMessage(e)
    // A port conflict is the one failure with an obvious next action, so
    // find out who's responsible rather than leaving the message as advice.
    if (error.value.includes('port')) {
      blocker.value = await store.portHolder(props.service.port).catch(() => null)
    }
  }
}

/** Kills the port's owner, then starts the service. Two steps on purpose —
 *  if freeing fails, that's what gets reported, not a confusing start error. */
async function freePortAndStart() {
  freeing.value = true
  error.value = null
  try {
    await store.freePort(props.service.port)
    blocker.value = null
    await store.start(props.service.id)
  } catch (e) {
    error.value = errorMessage(e)
  } finally {
    freeing.value = false
  }
}

async function onRestart() {
  error.value = null
  try {
    await store.restart(props.service.id)
  } catch (e) {
    error.value = errorMessage(e)
  }
}

/** Whether this service has state a hard kill could damage. Only the
 *  database does — nginx and php-cgi hold nothing worth flushing. */
const losesStateOnKill = computed(() => props.service.category === 'Database')

async function onForceStop() {
  menuOpen.value = false
  confirmingForceStop.value = false
  error.value = null
  try {
    await store.forceStop(props.service.id)
  } catch (e) {
    error.value = errorMessage(e)
  }
}

function requestForceStop() {
  // Nothing at stake for a stateless service — kill it without ceremony.
  if (!losesStateOnKill.value) {
    onForceStop()
    return
  }
  menuOpen.value = false
  confirmingForceStop.value = true
}
</script>

<template>
  <div
    class="rounded-2xl border border-neutral-200/80 bg-neutral-100/60 transition hover:border-neutral-300 dark:border-neutral-800 dark:bg-neutral-900/60"
  >
    <div class="flex items-center gap-3 p-3.5">
      <div
        class="flex h-9 w-9 shrink-0 items-center justify-center rounded-full text-sm font-semibold"
        :class="
          isRunning
            ? 'bg-emerald-100 text-emerald-700 dark:bg-emerald-500/15 dark:text-emerald-400'
            : 'bg-neutral-200/70 text-neutral-500 dark:bg-neutral-800 dark:text-neutral-400'
        "
      >
        {{ initial }}
      </div>

      <div class="min-w-0 flex-1">
        <div class="flex items-center gap-2">
          <span class="truncate font-semibold text-neutral-900 dark:text-neutral-100">{{
            service.name
          }}</span>
          <BasePill class="shrink-0">{{ service.category }}</BasePill>
        </div>
        <div class="mt-0.5 flex items-center gap-1.5 text-sm">
          <span
            class="h-1.5 w-1.5 rounded-full"
            :class="isRunning ? 'bg-emerald-500' : 'bg-neutral-400 dark:bg-neutral-600'"
          ></span>
          <span
            :class="
              isRunning ? 'font-medium text-emerald-600 dark:text-emerald-400' : 'text-neutral-500'
            "
          >
            {{ isRunning ? 'Running' : 'Stopped' }}
          </span>
        </div>
        <p v-if="error" class="mt-0.5 text-xs text-red-600 dark:text-red-400">
          {{ error }}
        </p>
      </div>

      <!-- Dropped on narrow windows so the controls never get pushed off-screen. -->
      <div
        v-if="isRunning && service.cpuHistory.length"
        class="hidden shrink-0 items-center gap-2 lg:flex"
      >
        <ServiceSparkline :values="service.cpuHistory" />
        <span class="font-mono text-xs whitespace-nowrap text-neutral-500">
          {{ service.cpuPercent }}% cpu
        </span>
      </div>

      <BasePill variant="mono" class="shrink-0">{{ service.version }}</BasePill>
      <BasePill variant="mono" class="shrink-0">:{{ service.port }}</BasePill>

      <button
        type="button"
        class="flex shrink-0 items-center gap-1.5 rounded-lg px-3.5 py-1.5 text-sm font-semibold transition disabled:opacity-50"
        :class="
          isRunning
            ? 'bg-red-100 text-red-600 hover:bg-red-200 dark:bg-red-500/15 dark:text-red-400 dark:hover:bg-red-500/25'
            : 'bg-red-600 text-white shadow-sm shadow-red-600/30 hover:bg-red-500'
        "
        :disabled="isPending"
        @click="onPrimaryAction"
      >
        <svg
          v-if="isRunning"
          viewBox="0 0 10 10"
          fill="currentColor"
          aria-hidden="true"
          class="h-2 w-2"
        >
          <rect width="10" height="10" rx="1.5" />
        </svg>
        <svg v-else viewBox="0 0 10 10" fill="currentColor" aria-hidden="true" class="h-2.5 w-2.5">
          <path d="M1.5 0.8 9 5 1.5 9.2Z" />
        </svg>
        {{ isRunning ? 'Stop' : 'Start' }}
      </button>

      <button
        type="button"
        title="Restart"
        class="flex h-8 w-8 shrink-0 items-center justify-center rounded-full border border-neutral-200 bg-white/60 text-neutral-500 transition hover:bg-white disabled:opacity-50 dark:border-neutral-700 dark:bg-neutral-800/60 dark:text-neutral-400 dark:hover:bg-neutral-800"
        :disabled="isPending"
        @click="onRestart"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="h-4 w-4">
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            d="M4.5 12a7.5 7.5 0 0 1 12.8-5.3L20 9M20 9V4M20 9h-5"
          />
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            d="M19.5 12a7.5 7.5 0 0 1-12.8 5.3L4 15m0 0v5m0-5h5"
          />
        </svg>
      </button>

      <!-- Force stop lives behind a menu rather than beside Stop: it's the
           exception, and putting it one click away keeps it from being hit
           by accident. Only offered while there's a process to kill. -->
      <div v-if="isRunning" class="relative shrink-0">
        <button
          type="button"
          title="More actions"
          class="flex h-8 w-8 items-center justify-center rounded-full border border-neutral-200 bg-white/60 text-neutral-500 transition hover:bg-white dark:border-neutral-700 dark:bg-neutral-800/60 dark:text-neutral-400 dark:hover:bg-neutral-800"
          @click="menuOpen = !menuOpen"
        >
          <svg viewBox="0 0 24 24" fill="currentColor" class="h-4 w-4">
            <circle cx="12" cy="5" r="1.6" />
            <circle cx="12" cy="12" r="1.6" />
            <circle cx="12" cy="19" r="1.6" />
          </svg>
        </button>

        <div v-if="menuOpen" class="fixed inset-0 z-10" @click="menuOpen = false" />

        <div
          v-if="menuOpen"
          class="absolute right-0 z-20 mt-2 w-56 overflow-hidden rounded-xl border border-neutral-200 bg-white shadow-xl dark:border-neutral-700 dark:bg-neutral-900"
        >
          <button
            type="button"
            class="flex w-full items-start gap-2.5 px-3.5 py-2.5 text-left transition hover:bg-neutral-50 disabled:opacity-50 dark:hover:bg-neutral-800/60"
            :disabled="isPending"
            @click="requestForceStop"
          >
            <svg
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
              class="mt-0.5 h-4 w-4 shrink-0 text-red-600 dark:text-red-400"
            >
              <path stroke-linecap="round" d="M18.4 5.6 5.6 18.4M5.6 5.6l12.8 12.8" />
            </svg>
            <span>
              <span class="block text-sm font-semibold text-red-600 dark:text-red-400">
                Force stop
              </span>
              <span class="block text-xs text-neutral-500">
                {{
                  losesStateOnKill
                    ? 'Kills it immediately — needs crash recovery'
                    : 'Kills the process immediately'
                }}
              </span>
            </span>
          </button>
        </div>
      </div>

      <button
        type="button"
        title="Toggle logs"
        class="flex h-8 w-8 shrink-0 items-center justify-center rounded-full border border-neutral-200 bg-white/60 text-neutral-500 transition hover:bg-white dark:border-neutral-700 dark:bg-neutral-800/60 dark:text-neutral-400 dark:hover:bg-neutral-800"
        @click="toggleExpanded"
      >
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          class="h-4 w-4 transition-transform"
          :class="expanded ? 'rotate-180' : ''"
        >
          <path stroke-linecap="round" stroke-linejoin="round" d="m19.5 8.25-7.5 7.5-7.5-7.5" />
        </svg>
      </button>
    </div>

    <!-- The way out of a port conflict. Shown only after a start actually
         failed on one, so it never invites killing something pre-emptively. -->
    <div
      v-if="blocker"
      class="mx-3.5 mb-3.5 rounded-xl border p-3.5"
      :class="
        blocker.kind === 'system'
          ? 'border-neutral-200 bg-neutral-50 dark:border-neutral-700 dark:bg-neutral-800/40'
          : 'border-amber-200 bg-amber-50 dark:border-amber-500/25 dark:bg-amber-500/10'
      "
    >
      <p class="text-sm font-semibold text-neutral-900 dark:text-neutral-100">
        {{ blocker.description }}
      </p>
      <p
        v-if="blocker.path"
        class="mt-1 truncate font-mono text-xs text-neutral-500"
        :title="blocker.path"
      >
        {{ blocker.path }}
      </p>

      <!-- A system process can't be killed, so it gets an explanation and no
           button — offering one that always fails would be worse than none. -->
      <p
        v-if="blocker.kind === 'system'"
        class="mt-2 text-xs text-neutral-600 dark:text-neutral-300"
      >
        This one has to be stopped as a Windows service; Rezure can't end it.
      </p>

      <div v-else class="mt-3 flex flex-wrap items-center gap-2">
        <button
          type="button"
          class="rounded-full bg-red-600 px-4 py-1.5 text-sm font-semibold text-white transition hover:bg-red-500 disabled:opacity-50"
          :disabled="freeing || isPending"
          @click="freePortAndStart"
        >
          {{ freeing ? 'Stopping…' : `Stop it and start ${service.name}` }}
        </button>
        <button
          type="button"
          class="rounded-full px-4 py-1.5 text-sm font-semibold text-neutral-600 dark:text-neutral-300"
          @click="blocker = null"
        >
          Cancel
        </button>
        <!-- Killing someone else's server is a different decision from
             reclaiming our own leftover, so it's labelled as one. -->
        <span v-if="blocker.kind === 'foreign'" class="text-xs text-amber-700 dark:text-amber-300">
          This isn't Rezure's — make sure you don't need it running.
        </span>
      </div>
    </div>

    <!-- Stated plainly rather than as a generic "are you sure": the cost of
         killing a database is specific, and the safe alternative is one
         button away. -->
    <div
      v-if="confirmingForceStop"
      class="mx-3.5 mb-3.5 rounded-xl border border-amber-200 bg-amber-50 p-3.5 dark:border-amber-500/25 dark:bg-amber-500/10"
    >
      <p class="text-sm font-semibold text-amber-900 dark:text-amber-200">
        Force stop {{ service.name }}?
      </p>
      <p class="mt-1 text-xs text-amber-800 dark:text-amber-300">
        It won't be given the chance to close its data directory, so the next start has to run crash
        recovery. Use <strong>Stop</strong> instead unless it's unresponsive.
      </p>
      <div class="mt-3 flex gap-2">
        <button
          type="button"
          class="rounded-full bg-red-600 px-4 py-1.5 text-sm font-semibold text-white transition hover:bg-red-500 disabled:opacity-50"
          :disabled="isPending"
          @click="onForceStop"
        >
          Force stop
        </button>
        <button
          type="button"
          class="rounded-full px-4 py-1.5 text-sm font-semibold text-amber-900 dark:text-amber-200"
          @click="confirmingForceStop = false"
        >
          Cancel
        </button>
      </div>
    </div>

    <ServiceLogPanel v-if="expanded" :service-id="service.id" />
  </div>
</template>
