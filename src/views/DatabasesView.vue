<script setup lang="ts">
import { computed, onActivated, ref } from 'vue'
import { open as openFileDialog } from '@tauri-apps/plugin-dialog'
import { useDatabasesStore } from '@/stores/databases'
import { useDbProfilesStore } from '@/stores/dbProfiles'
import { useDbConnectionsStore } from '@/stores/dbConnections'
import OpenInClientMenu from '@/components/databases/OpenInClientMenu.vue'
import NewDatabaseModal from '@/components/databases/NewDatabaseModal.vue'
import ImportSqlModal from '@/components/databases/ImportSqlModal.vue'
import DbTargetSwitcher from '@/components/databases/DbTargetSwitcher.vue'
import SearchInput from '@/components/common/SearchInput.vue'
import BusyOverlay from '@/components/common/BusyOverlay.vue'
import LeafLoader from '@/components/common/LeafLoader.vue'

const store = useDatabasesStore()
const profilesStore = useDbProfilesStore()
const connectionsStore = useDbConnectionsStore()

/** True while the page is reading a server Rezure doesn't run. Almost every
 *  statement this view makes — "no password", which project uses a schema,
 *  "start it from Services" — is only true of the local server. */
const remote = computed(() => store.server?.remote === true)
/** Writes are refused for the target: a read-only connection. */
const readOnly = computed(() => store.server?.readOnly === true)

const showNewDatabaseModal = ref(false)
const importFile = ref<string | null>(null)
const search = ref('')

/** Matched on name alone — collation and size are things you read once you've
 *  found the database, not things you go looking for it by. */
const filteredDatabases = computed(() => {
  const query = search.value.trim().toLowerCase()
  if (!query) return store.databases
  return store.databases.filter((db) => db.name.toLowerCase().includes(query))
})

const subtitle = computed(() => {
  const client = store.preferredClient
  const handoff = client ? `hand off to ${client.name}` : 'hand off to your SQL client'
  // The local promise ("never asks you for credentials") isn't one this page
  // can make about somebody else's server, so it isn't made.
  if (remote.value) {
    return `Reading ${store.server?.label ?? 'a remote server'} — list, export and ${handoff}.`
  }
  return `Import or Export your database with one click`
})

/** Binary units, matching what a database client would report. */
function formatSize(bytes: number) {
  if (bytes === 0) return '—'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  const exponent = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1)
  const value = bytes / 1024 ** exponent
  return `${value >= 10 || exponent === 0 ? Math.round(value) : value.toFixed(1)} ${units[exponent]}`
}

async function pickSqlFile() {
  const picked = await openFileDialog({
    multiple: false,
    directory: false,
    filters: [{ name: 'SQL dump', extensions: ['sql'] }],
  })
  if (typeof picked === 'string') importFile.value = picked
}

const ACTION_BUTTON_CLASS =
  'glass-btn flex h-9 shrink-0 items-center gap-1.5 rounded-full px-3.5 text-sm font-semibold text-neutral-700 transition select-none hover:text-neutral-900 disabled:opacity-40 dark:text-neutral-200 dark:hover:text-neutral-50'

// Kept-alive view: this runs on the first mount and on every return, and the
// store keeps the existing rows on screen while it refetches.
onActivated(() => {
  store.fetchAll()
})

/** The profile being switched to, by name rather than id. */
const switchingProfile = computed(() => {
  const id = profilesStore.switchingId
  if (!id) return null
  return profilesStore.profiles.find((profile) => profile.id === id)?.name ?? 'another profile'
})

/**
 * The three slow paths on this page, most disruptive first.
 *
 * Switching a profile wins because it stops and restarts the server itself,
 * so it invalidates everything else on screen. Import beats export because
 * it's the one the user just confirmed in a dialog. Creating a database is a
 * single fast statement and keeps the modal button as its only feedback.
 */
const busyLabel = computed(() => {
  if (switchingProfile.value) return `Switching to ${switchingProfile.value}…`
  if (store.importingInto) return `Importing into ${store.importingInto}…`
  if (store.busy) return `Exporting ${store.busy}…`
  return ''
})

const busyDetail = computed(() => {
  if (switchingProfile.value) return 'Pointing the server at a different data directory.'
  if (store.importingInto) return `Reading the dump into ${store.server?.label ?? 'the server'}.`
  // Worth saying for a remote dump: it crosses the network and can take
  // minutes, where a local one is effectively instant.
  return remote.value
    ? `Pulling a .sql dump from ${store.server?.label} to C:\\rezure\\dumps.`
    : 'Writing a .sql dump to C:\\rezure\\dumps.'
})

/** Progress and Cancel are an export's alone — switching a profile and
 *  importing a file have no byte count to show one for, and killing either
 *  mid-flight is a different, riskier thing than killing a dump. */
const exporting = computed(
  () => store.busy !== null && !switchingProfile.value && !store.importingInto,
)
</script>

<template>
  <!-- Fills the main area exactly and scrolls the table inside itself, so the
       heading, connection banner and search stay put however many databases
       there are. -->
  <section class="flex h-full flex-col">
    <div class="flex shrink-0 items-start justify-between gap-4">
      <div class="min-w-0">
        <h1 class="text-[28px] leading-tight font-bold tracking-tight">Databases</h1>
        <p class="mt-1 text-sm text-neutral-500">{{ subtitle }}</p>
      </div>

      <div class="flex shrink-0 items-center gap-2">
        <DbTargetSwitcher />

        <!-- The list is only refetched when the page is entered, and the page
             is kept alive — so after starting MariaDB from Services there is
             otherwise no way to re-read it without navigating away and back. -->
        <button
          type="button"
          class="glass-btn flex h-9.5 w-9.5 shrink-0 items-center justify-center rounded-full text-neutral-600 transition hover:text-neutral-900 disabled:opacity-50 dark:text-neutral-300 dark:hover:text-neutral-50"
          :disabled="store.refreshing"
          title="Refresh the database list"
          aria-label="Refresh the database list"
          @click="store.fetchAll"
        >
          <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            class="h-4 w-4"
            :class="store.refreshing ? 'animate-spin' : ''"
          >
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15"
            />
          </svg>
        </button>

        <button
          type="button"
          class="glass-accent flex shrink-0 items-center gap-2 rounded-full px-4 py-2 text-sm font-semibold transition disabled:cursor-not-allowed disabled:opacity-50"
          :disabled="store.serverDown || readOnly"
          :title="readOnly ? `${store.server?.label} is read-only` : ''"
          @click="showNewDatabaseModal = true"
        >
          <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2.5"
            class="h-4 w-4"
          >
            <path stroke-linecap="round" d="M12 5v14M5 12h14" />
          </svg>
          New database
        </button>
      </div>
    </div>

    <p
      v-if="profilesStore.notice || connectionsStore.notice"
      class="mt-3 shrink-0 text-sm text-neutral-600 dark:text-neutral-300"
    >
      {{ profilesStore.notice ?? connectionsStore.notice }}
    </p>
    <p
      v-if="profilesStore.error || connectionsStore.error"
      class="mt-3 shrink-0 text-sm text-red-600 dark:text-red-400"
    >
      {{ profilesStore.error ?? connectionsStore.error }}
    </p>

    <!-- A stopped MariaDB isn't an error the user made, so it gets an
         explanation and a way forward rather than a raw client message. -->
    <div
      v-if="store.serverDown"
      class="mt-4 shrink-0 rounded-2xl border border-amber-300/60 bg-amber-100/50 px-4 py-3 text-sm text-amber-900 dark:border-amber-500/25 dark:bg-amber-500/10 dark:text-amber-200"
    >
      <template v-if="remote">
        Couldn't reach {{ store.server?.label }} at
        <span class="font-mono">{{ store.server?.host }}:{{ store.server?.port }}</span
        >. Check the host is up and reachable from this machine, then
        <button type="button" class="font-semibold underline" @click="store.fetchAll">retry</button
        >.
      </template>
      <template v-else>
        MariaDB isn't running, so there's nothing to list yet. Start it from
        <RouterLink to="/" class="font-semibold underline">Services</RouterLink>, then
        <button type="button" class="font-semibold underline" @click="store.fetchAll">retry</button
        >.
      </template>
    </div>
    <p v-else-if="store.error" class="mt-4 shrink-0 text-sm text-red-600 dark:text-red-400">
      {{ store.error }}
    </p>

    <p
      v-if="store.notice"
      class="mt-3 flex shrink-0 flex-wrap items-center gap-2 text-sm text-neutral-500"
    >
      <span class="min-w-0 truncate font-mono text-xs">{{ store.notice }}</span>
      <button
        type="button"
        class="shrink-0 font-semibold text-accent-600 underline dark:text-accent-400"
        @click="store.openDumpsFolder"
      >
        Show folder
      </button>
    </p>

    <NewDatabaseModal v-if="showNewDatabaseModal" @close="showNewDatabaseModal = false" />
    <ImportSqlModal v-if="importFile" :file="importFile" @close="importFile = null" />

    <BusyOverlay
      :show="busyLabel !== ''"
      :label="busyLabel"
      :detail="busyDetail"
      :percent="exporting ? store.exportPercent : null"
      :on-cancel="exporting ? store.cancelExport : undefined"
    />

    <!-- Search, import and the totals sit above the table so the table itself
         can take the rest of the height and scroll inside its own frame. -->
    <div
      v-if="!store.serverDown && store.databases.length > 0"
      class="mt-5 flex shrink-0 flex-wrap items-center gap-3"
    >
      <SearchInput v-model="search" placeholder="Search databases" class="w-full max-w-md" />

      <!-- Not merely disabled: on a read-only connection an import is not a
           thing the user can do here at all, and a greyed button invites a
           hunt for the setting that would enable it. -->
      <button
        v-if="!readOnly"
        type="button"
        class="glass-btn flex shrink-0 items-center gap-2 rounded-full px-4 py-2 text-sm font-semibold text-neutral-700 transition dark:text-neutral-200"
        @click="pickSqlFile"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="h-4 w-4">
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            d="M12 4v11m0 0-4-4m4 4 4-4M5 19h14"
          />
        </svg>
        Import .sql
      </button>

      <span class="shrink-0 text-sm text-neutral-500">
        {{ filteredDatabases.length }} databases · {{ store.totalTables }} tables total
      </span>
    </div>

    <!-- First load only, so it never replaces a list that's already correct.
         Inline rather than the overlay: there is nothing on the page yet to
         float above, and this is the page's own empty state. -->
    <div
      v-if="store.loading && store.databases.length === 0"
      class="mt-12 flex shrink-0 justify-center"
    >
      <LeafLoader :size="52" label="Reading schemas…" />
    </div>

    <!-- Import is offered here too: with no databases at all, the toolbar
         above is hidden, and a dump is the likeliest way to get started. -->
    <div
      v-else-if="store.databases.length === 0 && !store.serverDown && !store.error"
      class="mt-8 shrink-0 text-center text-sm text-neutral-500"
    >
      <template v-if="readOnly">
        No databases on {{ store.server?.label }} — or none this user can see.
      </template>
      <template v-else>
        No databases yet — create one, or
        <button type="button" class="font-semibold text-accent-600 underline" @click="pickSqlFile">
          import a .sql dump</button
        >.
      </template>
    </div>

    <div
      v-else-if="filteredDatabases.length === 0 && store.databases.length > 0"
      class="mt-8 shrink-0 text-center text-sm text-neutral-500"
    >
      No databases match "{{ search }}".
    </div>

    <div
      v-else-if="filteredDatabases.length > 0"
      class="glass mt-5 flex min-h-0 flex-1 flex-col overflow-hidden rounded-2xl"
    >
      <!-- Column headings stay put; only the rows below them move. -->
      <div
        class="glass-divider flex shrink-0 items-center gap-3 border-b bg-white/50 px-5 py-3 text-[11px] font-semibold tracking-wide text-neutral-400 uppercase dark:bg-white/4"
      >
        <span class="flex-1">Database</span>
        <span class="w-20 shrink-0 text-right">Tables</span>
        <span class="w-24 shrink-0 text-right">Size</span>
        <span v-if="!remote" class="w-40 shrink-0 pl-6">Used by</span>
        <span class="w-52 shrink-0 text-right">Actions</span>
      </div>

      <div class="min-h-0 flex-1 overflow-y-auto">
        <div
          v-for="db in filteredDatabases"
          :key="db.name"
          class="glass-divider flex items-center gap-3 border-b px-5 py-3.5 transition last:border-b-0 hover:bg-white/50 dark:hover:bg-white/5"
        >
          <div class="min-w-0 flex-1">
            <p class="truncate font-mono font-semibold text-neutral-900 dark:text-neutral-100">
              {{ db.name }}
            </p>
            <p class="truncate font-mono text-xs text-neutral-500">{{ db.collation }}</p>
          </div>

          <span
            class="w-20 shrink-0 text-right font-mono text-sm text-neutral-600 dark:text-neutral-300"
          >
            {{ db.tableCount }}
          </span>
          <span
            class="w-24 shrink-0 text-right font-mono text-sm text-neutral-600 dark:text-neutral-300"
          >
            {{ formatSize(db.sizeBytes) }}
          </span>
          <!-- Hidden rather than dashed out for a remote server: a local
               project folder says nothing about a schema on somebody else's
               machine, so there is no answer to show. -->
          <span v-if="!remote" class="w-40 shrink-0 truncate pl-6 font-mono text-xs">
            <span v-if="db.usedBy" class="text-neutral-600 dark:text-neutral-300">{{
              db.usedBy
            }}</span>
            <span v-else class="text-neutral-300 dark:text-neutral-600">—</span>
          </span>

          <div class="flex w-52 shrink-0 items-center justify-end gap-1.5">
            <OpenInClientMenu :database="db.name" />
            <button
              type="button"
              :class="ACTION_BUTTON_CLASS"
              :disabled="store.busy === db.name"
              :title="`Export ${db.name} to a timestamped .sql file`"
              @click="store.exportDatabase(db.name)"
            >
              <svg
                v-if="store.busy === db.name"
                viewBox="0 0 24 24"
                fill="none"
                class="h-4 w-4 animate-spin"
              >
                <circle
                  cx="12"
                  cy="12"
                  r="9"
                  stroke="currentColor"
                  stroke-width="2.5"
                  opacity="0.25"
                />
                <path
                  d="M21 12a9 9 0 0 0-9-9"
                  stroke="currentColor"
                  stroke-width="2.5"
                  stroke-linecap="round"
                />
              </svg>
              <svg
                v-else
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                class="h-4 w-4"
              >
                <path
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  d="M12 20V9m0 0-4 4m4-4 4 4M5 5h14"
                />
              </svg>
              {{ store.busy === db.name ? 'Exporting…' : 'Export' }}
            </button>
          </div>
        </div>
      </div>
    </div>
  </section>
</template>
