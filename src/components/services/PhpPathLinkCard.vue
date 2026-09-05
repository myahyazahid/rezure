<script setup lang="ts">
import { computed, onMounted } from 'vue'
import { usePhpStore } from '@/stores/php'

const store = usePhpStore()

const status = computed(() => store.pathStatus)
const enabled = computed(() => status.value?.onPath === true)

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
          Puts the active version on your PATH, so <code class="font-mono">php</code> works in every
          terminal — not only the ones Rezure opens.
        </p>
      </div>

      <button
        type="button"
        role="switch"
        :aria-checked="enabled"
        :aria-label="`Use Rezure's PHP everywhere`"
        class="relative h-6 w-11 shrink-0 rounded-full transition disabled:opacity-50"
        :class="enabled ? 'bg-red-600' : 'bg-neutral-200 dark:bg-neutral-700'"
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
      <div
        class="flex items-center gap-2 rounded-lg bg-neutral-100 px-2.5 py-1.5 font-mono text-xs dark:bg-neutral-800/60"
      >
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

      <!-- The distinction that actually trips people up: adding the entry
           can't reach a shell that already started, but re-pointing the
           link later can, because the entry is in its PATH by then. -->
      <p class="text-xs text-neutral-500">
        Open a <strong>new</strong> terminal to pick this up — ones already open keep the PATH they
        started with. After that, switching versions reaches them too.
      </p>
      <p v-if="!status.inSync" class="text-xs text-amber-700 dark:text-amber-300">
        The link is out of sync — switch a version to re-point it.
      </p>
    </div>

    <!-- Stated before enabling, not discovered afterwards: this is the one
         thing in Rezure that changes something outside the app. -->
    <p
      v-else-if="conflictSummary"
      class="mt-3 rounded-lg bg-amber-50 px-2.5 py-2 text-xs text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
    >
      <strong>{{ conflictSummary }}</strong> already provides <code class="font-mono">php</code>.
      Turning this on puts Rezure ahead of it; turning it off hands it straight back.
    </p>
  </div>
</template>
