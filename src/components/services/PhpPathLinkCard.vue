<script setup lang="ts">
import { computed, onMounted } from 'vue'
import { usePhpStore } from '@/stores/php'
import { useComposerStore } from '@/stores/composer'
import { useNodeStore } from '@/stores/node'

const store = usePhpStore()
const composer = useComposerStore()
const node = useNodeStore()

const status = computed(() => store.pathStatus)
const enabled = computed(() => status.value?.onPath === true)

/** The runtimes that ride on this switch besides PHP, one row each. A
 *  `null` target means nothing of it is installed yet. */
const toolRows = computed(() => [
  {
    command: 'composer',
    target: status.value?.composerTarget ?? null,
    version: composer.active?.version,
    missing: 'install a Composer version to use it',
  },
  {
    command: 'node',
    target: status.value?.nodeTarget ?? null,
    version: node.active?.version,
    missing: 'install a Node.js version to use it',
  },
])

/** Just the tool's folder name, so the warning can say "Laragon" instead of
 *  making the user parse a path. */
function ownerOf(path: string) {
  const lower = path.toLowerCase()
  if (lower.includes('laragon')) return 'Laragon'
  if (lower.includes('xampp')) return 'XAMPP'
  if (lower.includes('wamp')) return 'WAMP'
  return null
}

const conflictSummary = computed(() => {
  const conflicts = status.value?.conflicts ?? []
  if (conflicts.length === 0) return null
  const owners = [...new Set(conflicts.map(ownerOf).filter(Boolean))]
  return owners.length > 0 ? owners.join(' and ') : `${conflicts.length} other PHP install(s)`
})

onMounted(() => {
  if (!store.pathStatus) store.fetchPathStatus()
  if (composer.versions.length === 0) void composer.fetchVersions()
  if (node.versions.length === 0) void node.fetchVersions()
})
</script>

<template>
  <div v-if="status" class="p-4">
    <div class="flex items-start justify-between gap-4">
      <div class="min-w-0">
        <p class="font-semibold text-neutral-900 dark:text-neutral-100">
          Use Rezure's PHP everywhere
        </p>
        <p class="mt-0.5 text-xs text-neutral-500">
          Puts the active PHP, Composer, and Node.js on your PATH, so
          <code class="font-mono">php</code>, <code class="font-mono">composer</code>,
          <code class="font-mono">node</code>, and <code class="font-mono">npm</code> work in every
          terminal - not only the ones Rezure opens.
        </p>
      </div>

      <button
        type="button"
        role="switch"
        :aria-checked="enabled"
        :aria-label="`Use Rezure's PHP everywhere`"
        class="relative h-6 w-11 shrink-0 rounded-full transition disabled:opacity-50"
        :class="enabled ? 'bg-accent-600' : 'bg-neutral-900/15 dark:bg-white/15'"
        :disabled="store.pathBusy"
        @click="store.setPathLink(!enabled)"
      >
        <span
          class="absolute top-0.5 h-5 w-5 rounded-full bg-white shadow transition"
          :class="enabled ? 'left-5' : 'left-0.5'"
        />
      </button>
    </div>

    <div v-if="enabled" class="mt-3 flex flex-col gap-1.5">
      <div class="glass-inset flex items-center gap-2 rounded-lg px-2.5 py-1.5 font-mono text-xs">
        <span class="shrink-0 text-neutral-500">php</span>
        <span class="shrink-0 text-neutral-400">→</span>
        <span class="shrink-0 font-semibold text-emerald-600 dark:text-emerald-400">
          {{ store.active?.version ?? 'the active version' }}
        </span>
        <span
          v-if="status.target"
          class="ml-auto min-w-0 truncate text-neutral-400"
          :title="status.target"
        >
          {{ status.target }}
        </span>
      </div>
      <div
        v-for="row in toolRows"
        :key="row.command"
        class="glass-inset flex items-center gap-2 rounded-lg px-2.5 py-1.5 font-mono text-xs"
      >
        <span class="shrink-0 text-neutral-500">{{ row.command }}</span>
        <span class="shrink-0 text-neutral-400">→</span>
        <template v-if="row.target">
          <span class="shrink-0 font-semibold text-emerald-600 dark:text-emerald-400">
            {{ row.version ?? 'the active version' }}
          </span>
          <span class="ml-auto min-w-0 truncate text-neutral-400" :title="row.target">
            {{ row.target }}
          </span>
        </template>
        <span v-else class="text-neutral-400">{{ row.missing }}</span>
      </div>

      <!-- The distinction that actually trips people up: adding the entry
           can't reach a shell that already started, but re-pointing the
           link later can, because the entry is in its PATH by then. -->
      <p class="text-xs text-neutral-500">
        Open a <strong>new</strong> terminal to pick this up - ones already open keep the PATH they
        started with. After that, switching versions reaches them too.
      </p>
      <p v-if="!status.inSync" class="text-xs text-amber-700 dark:text-amber-300">
        The link is out of sync - switch a version to re-point it.
      </p>
    </div>

    <!-- Stated before enabling, not discovered afterwards: this is the one
         thing in Rezure that changes something outside the app. -->
    <p
      v-else-if="conflictSummary"
      class="mt-3 rounded-lg bg-amber-100/50 px-2.5 py-2 text-xs text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
    >
      <strong>{{ conflictSummary }}</strong> already provides <code class="font-mono">php</code>.
      Turning this on puts Rezure ahead of it; turning it off hands it straight back.
    </p>
  </div>
</template>
