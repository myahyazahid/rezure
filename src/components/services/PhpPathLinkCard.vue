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
  <div
    v-if="status"
    class="rounded-2xl border p-4 transition"
    :class="
      enabled
        ? 'border-emerald-200 bg-emerald-50/60 dark:border-emerald-500/25 dark:bg-emerald-500/10'
        : 'border-neutral-200/80 bg-neutral-100/60 dark:border-neutral-800 dark:bg-neutral-900/60'
    "
  >
    <div class="flex items-start gap-3">
      <span
        class="flex h-9 w-9 shrink-0 items-center justify-center rounded-full"
        :class="
          enabled
            ? 'bg-emerald-100 text-emerald-700 dark:bg-emerald-500/15 dark:text-emerald-400'
            : 'bg-neutral-200/70 text-neutral-500 dark:bg-neutral-800 dark:text-neutral-400'
        "
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="h-4 w-4">
          <path stroke-linecap="round" stroke-linejoin="round" d="M4 7h6l2 3h8M4 7v10h16V10" />
        </svg>
      </span>

      <div class="min-w-0 flex-1">
        <p class="font-semibold text-neutral-900 dark:text-neutral-100">
          Use Rezure's PHP everywhere
        </p>
        <p class="mt-0.5 text-sm text-neutral-500">
          Puts the active version on your PATH, so <code class="font-mono">php</code> works in every
          terminal — not only the ones Rezure opens. Once it's on, switching versions takes effect
          even in terminals you already have open.
        </p>
      </div>

      <button
        type="button"
        class="shrink-0 rounded-full px-4 py-2 text-sm font-semibold transition disabled:opacity-50"
        :class="
          enabled
            ? 'border border-neutral-200 bg-white text-neutral-700 hover:bg-neutral-100 dark:border-neutral-700 dark:bg-neutral-800/60 dark:text-neutral-200'
            : 'bg-red-600 text-white shadow-sm shadow-red-600/30 hover:bg-red-500'
        "
        :disabled="store.pathBusy"
        @click="store.setPathLink(!enabled)"
      >
        <template v-if="store.pathBusy">Working…</template>
        <template v-else>{{ enabled ? 'Disable' : 'Enable' }}</template>
      </button>
    </div>

    <!-- Stated before enabling, not discovered afterwards: this is the one
         thing in Rezure that changes something outside the app. -->
    <p
      v-if="!enabled && conflictSummary"
      class="mt-3 rounded-xl bg-amber-50 px-3 py-2 text-xs text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
    >
      <strong>{{ conflictSummary }}</strong> already provides <code class="font-mono">php</code> on
      your PATH. Enabling this puts Rezure ahead of it, so
      <code class="font-mono">php</code> system-wide becomes Rezure's. Disabling hands it straight
      back.
      <span class="mt-1 block truncate font-mono text-[11px] opacity-70">
        {{ status.conflicts.join(' · ') }}
      </span>
    </p>

    <div v-if="enabled" class="mt-3 flex flex-col gap-1 text-xs">
      <p class="text-emerald-700 dark:text-emerald-400">
        <code class="font-mono">php</code> resolves to
        <strong>{{ store.active?.version ?? 'the active version' }}</strong>
        <span v-if="!status.inSync" class="text-amber-700 dark:text-amber-300">
          — the link is out of sync, switch a version to re-point it.
        </span>
      </p>
      <!-- The distinction that actually trips people up: adding the entry
           can't reach a shell that already started, but re-pointing the
           link later can, because the entry is in its PATH by then. -->
      <p class="text-neutral-500">
        Open a <strong>new</strong> terminal to pick this up — ones already open kept the PATH they
        started with. After that, switching versions reaches them too.
      </p>
      <p class="truncate font-mono text-neutral-400" :title="status.linkDir">
        {{ status.linkDir }}
      </p>
      <p v-if="status.target" class="truncate font-mono text-neutral-400" :title="status.target">
        → {{ status.target }}
      </p>
    </div>
  </div>
</template>
