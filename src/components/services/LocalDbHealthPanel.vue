<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { LocalDbHealth } from '@/types/service'
import { useServicesStore } from '@/stores/services'

/**
 * Why SQL Server LocalDB won't start, and the way out — under its row on
 * the Services page. The checks themselves are in Rust
 * (`services::localdb_health`); this only asks and renders.
 *
 * A disk SQL Server can't run on is shown as soon as the page loads, since
 * every start would fail on it. Recreating the instance is only offered
 * after a start actually failed for some other reason.
 */
const props = defineProps<{
  /** A start just failed — re-check, and offer recreating the instance. */
  startFailed: boolean
}>()

const emit = defineEmits<{
  /** The instance was recreated and is running. */
  recovered: []
}>()

const store = useServicesStore()

const health = ref<LocalDbHealth | null>(null)
const busy = ref(false)
const error = ref<string | null>(null)
/** Recreating detaches databases, so it asks first. */
const confirmingRecreate = ref(false)

async function refresh() {
  try {
    health.value = await store.localDbHealth()
  } catch {
    // The check is advisory; a failure to run it shows nothing.
    health.value = null
  }
}

onMounted(refresh)

watch(
  () => props.startFailed,
  (failed) => {
    if (failed) refresh()
    else confirmingRecreate.value = false
  },
)

const sectorLabel = computed(() =>
  health.value?.sectorBytes
    ? `${health.value.sectorBytes.toLocaleString()} bytes`
    : 'more than 4,096 bytes',
)

const diskProblem = computed(() => {
  const verdict = health.value?.verdict
  return verdict === 'needsFix' || verdict === 'restartPending' || verdict === 'fixIneffective'
})

/** Recreate is the answer only when the disk isn't the problem. */
const offerRecreate = computed(() => props.startFailed && !!health.value && !diskProblem.value)

function errorMessage(e: unknown): string {
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  return 'Something went wrong.'
}

async function applyFix() {
  busy.value = true
  error.value = null
  try {
    health.value = await store.fixLocalDbDisk()
  } catch (e) {
    error.value = errorMessage(e)
  } finally {
    busy.value = false
  }
}

async function recreate() {
  busy.value = true
  error.value = null
  try {
    await store.recreateLocalDb()
    confirmingRecreate.value = false
    emit('recovered')
  } catch (e) {
    error.value = errorMessage(e)
    await refresh()
  } finally {
    busy.value = false
  }
}

/** Microsoft's write-up of the sector size problem and its workaround. */
const ARTICLE_URL =
  'https://learn.microsoft.com/en-us/troubleshoot/sql/database-engine/database-file-operations/troubleshoot-os-4kb-disk-sector-size'

async function openArticle() {
  try {
    await invoke('open_external_link', { url: ARTICLE_URL })
  } catch (e) {
    error.value = errorMessage(e)
  }
}

function askRecreate() {
  confirmingRecreate.value = true
}

function cancelRecreate() {
  confirmingRecreate.value = false
}
</script>

<template>
  <div
    v-if="diskProblem || offerRecreate"
    class="mx-3.5 mb-3.5 rounded-xl border border-amber-300/60 bg-amber-100/50 p-3.5 dark:border-amber-500/25 dark:bg-amber-500/10"
  >
    <!-- The disk needs Microsoft's workaround. -->
    <template v-if="health?.verdict === 'needsFix'">
      <p class="text-sm font-semibold text-amber-900 dark:text-amber-200">
        This disk needs a compatibility fix before SQL Server can run
      </p>
      <p class="mt-1 text-xs text-amber-800 dark:text-amber-300">
        Windows reports a disk sector size of {{ sectorLabel }}. SQL Server only works with 512 or
        4,096, so it crashes while starting, whatever its version. Microsoft's fix is a registry
        setting that makes Windows report 4,096 again, the same as Windows 10 did.
      </p>
      <p class="mt-1 text-xs text-amber-800 dark:text-amber-300">
        Windows asks for administrator permission first, and you'll need to restart Windows
        afterwards.
      </p>
      <div class="mt-3 flex flex-wrap items-center gap-2">
        <button
          type="button"
          class="glass-accent rounded-full px-4 py-1.5 text-sm font-semibold transition disabled:opacity-50"
          :disabled="busy"
          @click="applyFix"
        >
          {{ busy ? 'Applying…' : 'Fix disk compatibility' }}
        </button>
        <button
          type="button"
          class="text-xs font-semibold text-amber-800 underline dark:text-amber-300"
          @click="openArticle"
        >
          Microsoft's article
        </button>
      </div>
    </template>

    <!-- Fix set, not yet in effect. -->
    <template v-else-if="health?.verdict === 'restartPending'">
      <p class="text-sm font-semibold text-amber-900 dark:text-amber-200">
        Restart Windows to finish the disk fix
      </p>
      <p class="mt-1 text-xs text-amber-800 dark:text-amber-300">
        The fix is in place, but Windows keeps reporting {{ sectorLabel }} until it restarts. After
        restarting, start SQL Server again.
      </p>
    </template>

    <!-- Fix set before this boot, and the disk still reports too much. -->
    <template v-else-if="health?.verdict === 'fixIneffective'">
      <p class="text-sm font-semibold text-amber-900 dark:text-amber-200">
        The disk fix didn't take effect on this disk
      </p>
      <p class="mt-1 text-xs text-amber-800 dark:text-amber-300">
        Windows still reports {{ sectorLabel }} after restarting with the fix in place. The fix only
        reaches Windows' own NVMe driver, so this disk probably uses a driver from its manufacturer.
        Updating or replacing that storage driver is what's left.
      </p>
    </template>

    <!-- Not the disk: an instance left broken by an earlier failure. -->
    <template v-else-if="offerRecreate">
      <p class="text-sm font-semibold text-amber-900 dark:text-amber-200">
        The disk is fine. The instance itself may be broken
      </p>
      <p class="mt-1 text-xs text-amber-800 dark:text-amber-300">
        An earlier failed start, or a SQL Server update, can leave the instance's system databases
        unusable. Rezure can delete it and create a fresh one.
      </p>
      <p class="mt-1 text-xs text-amber-800 dark:text-amber-300">
        If it keeps crashing on this machine, the SQL Server release itself may not run here.
        Install an older one from <strong>Install version</strong>, then switch to it for SQL Server
        LocalDB on the <strong>Switch</strong> page.
      </p>
      <template v-if="confirmingRecreate">
        <p class="mt-2 text-xs font-semibold text-amber-900 dark:text-amber-200">
          Databases you created in it get detached. Their files stay in
          <span class="font-mono">C:\rezure\data\mssql</span> and can be attached again.
        </p>
        <div class="mt-3 flex flex-wrap gap-2">
          <button
            type="button"
            class="glass-accent rounded-full px-4 py-1.5 text-sm font-semibold text-red-600 transition disabled:opacity-50 dark:text-red-400"
            :disabled="busy"
            @click="recreate"
          >
            {{ busy ? 'Recreating…' : 'Delete and recreate' }}
          </button>
          <button
            type="button"
            class="rounded-full px-4 py-1.5 text-sm font-semibold text-amber-900 dark:text-amber-200"
            :disabled="busy"
            @click="cancelRecreate"
          >
            Cancel
          </button>
        </div>
      </template>
      <div v-else class="mt-3">
        <button
          type="button"
          class="glass-accent rounded-full px-4 py-1.5 text-sm font-semibold transition disabled:opacity-50"
          @click="askRecreate"
        >
          Recreate instance
        </button>
      </div>
    </template>

    <p v-if="error" class="mt-2 text-xs text-red-600 dark:text-red-400">{{ error }}</p>
  </div>
</template>
