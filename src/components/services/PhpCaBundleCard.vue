<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { usePhpStore } from '@/stores/php'

const store = usePhpStore()

/** Set after a download from here: whether it created the bundle (running
 *  PHP needs a restart to see it) or refreshed an existing one. */
const result = ref<{ created: boolean } | null>(null)

async function update() {
  const created = await store.updateCaBundle()
  if (created !== null) result.value = { created }
}

onMounted(() => {
  if (!store.caBundle) store.fetchCaBundle()
})
</script>

<template>
  <div v-if="store.caBundle" class="p-4">
    <div class="flex items-start justify-between gap-4">
      <div class="min-w-0">
        <p class="font-semibold text-neutral-900 dark:text-neutral-100">CA certificates</p>
        <p class="mt-0.5 text-xs text-neutral-500">
          What PHP checks HTTPS certificates against. The PHP zip ships none, so without this every
          outbound call fails with <code class="font-mono">cURL error 60</code>.
        </p>
      </div>

      <button
        type="button"
        class="glass-btn shrink-0 rounded-full px-4 py-1.5 text-xs font-semibold text-neutral-700 transition disabled:cursor-wait disabled:opacity-60 dark:text-neutral-200"
        :disabled="store.updatingCaBundle"
        @click="update"
      >
        <template v-if="store.updatingCaBundle">Downloading…</template>
        <template v-else-if="store.caBundle.installed">Update</template>
        <template v-else>Download</template>
      </button>
    </div>

    <div class="mt-3 flex flex-col gap-1.5">
      <template v-if="store.caBundle.installed">
        <p class="text-xs text-neutral-500">
          <template v-if="store.caBundle.mozillaDate">
            Mozilla's root certificates as of
            <strong class="font-semibold">{{ store.caBundle.mozillaDate }}</strong
            >, from curl.se.
          </template>
          <template v-else>A bundle Rezure didn't put there — kept as it is.</template>
        </p>
        <p
          class="glass-inset truncate rounded-lg px-2.5 py-1.5 font-mono text-xs text-neutral-500"
          :title="store.caBundle.path"
        >
          {{ store.caBundle.path }}
        </p>
      </template>
      <p
        v-else
        class="rounded-xl bg-amber-100/50 px-3 py-2 text-xs text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
      >
        Not installed — HTTPS calls from PHP can't be verified.
      </p>

      <p
        v-if="result"
        class="rounded-xl bg-emerald-100/50 px-3 py-2 text-xs text-emerald-800 dark:bg-emerald-500/10 dark:text-emerald-300"
      >
        <template v-if="result.created">
          Installed. Restart PHP for running sites to pick it up.
        </template>
        <template v-else>Updated. Running sites use it from the next request.</template>
      </p>
    </div>
  </div>
</template>
