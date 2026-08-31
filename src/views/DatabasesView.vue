<script setup lang="ts">
import { computed, onActivated, ref } from 'vue'
import { open as openFileDialog } from '@tauri-apps/plugin-dialog'
import { useDatabasesStore } from '@/stores/databases'
import { useDbProfilesStore } from '@/stores/dbProfiles'
import OpenInClientMenu from '@/components/databases/OpenInClientMenu.vue'
import NewDatabaseModal from '@/components/databases/NewDatabaseModal.vue'
import ImportSqlModal from '@/components/databases/ImportSqlModal.vue'
import DbProfileSwitcher from '@/components/databases/DbProfileSwitcher.vue'
import SearchInput from '@/components/common/SearchInput.vue'
import BusyOverlay from '@/components/common/BusyOverlay.vue'
import LeafLoader from '@/components/common/LeafLoader.vue'

const store = useDatabasesStore()
const profilesStore = useDbProfilesStore()

const showNewDatabaseModal = ref(false)
const importFile = ref<string | null>(null)
const copiedDsn = ref(false)
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
  return client
    ? `Create, export and hand off to ${client.name} — Rezure never asks you for credentials.`
    : 'Create, export and hand off to your SQL client — Rezure never asks you for credentials.'
})

/** Binary units, matching what a database client would report. */
function formatSize(bytes: number) {
  if (bytes === 0) return '—'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  const exponent = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1)
  const value = bytes / 1024 ** exponent
  return `${value >= 10 || exponent === 0 ? Math.round(value) : value.toFixed(1)} ${units[exponent]}`
}

async function copyDsn() {
  if (!store.server) return
  await navigator.clipboard.writeText(store.server.dsn)
  copiedDsn.value = true
  window.setTimeout(() => (copiedDsn.value = false), 1500)
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
  'flex h-9 shrink-0 items-center gap-1.5 rounded-full border border-neutral-200 bg-white px-3.5 text-sm font-semibold text-neutral-700 transition select-none hover:border-neutral-300 hover:text-neutral-900 disabled:opacity-40 dark:border-neutral-700 dark:bg-neutral-800/60 dark:text-neutral-200 dark:hover:text-neutral-50'

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
  if (store.importingInto) return 'Reading the dump into the server.'
  return 'Writing a .sql dump to ~/rezure/dumps.'
})
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
        <DbProfileSwitcher />

        <!-- The list is only refetched when the page is entered, and the page
             is kept alive — so after starting MariaDB from Services there is
             otherwise no way to re-read it without navigating away and back. -->
        <button
          type="button"
          class="flex h-10 w-10 shrink-0 items-center justify-center rounded-full border border-neutral-200 bg-white/70 text-neutral-600 transition hover:bg-white hover:text-neutral-900 disabled:opacity-50 dark:border-neutral-700 dark:bg-neutral-900/60 dark:text-neutral-300 dark:hover:bg-neutral-800 dark:hover:text-neutral-50"
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
          class="flex shrink-0 items-center gap-2 rounded-full bg-red-600 px-5 py-2.5 text-sm font-semibold text-white shadow-lg shadow-red-500/40 transition hover:bg-red-500 disabled:cursor-not-allowed disabled:opacity-50"
          :disabled="store.serverDown"
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
      v-if="profilesStore.notice"
      class="mt-3 shrink-0 text-sm text-neutral-600 dark:text-neutral-300"
    >
      {{ profilesStore.notice }}
    </p>
    <p v-if="profilesStore.error" class="mt-3 shrink-0 text-sm text-red-600 dark:text-red-400">
      {{ profilesStore.error }}
    </p>

    <!-- The connection, stated once and copyable — so nothing else in the
         app has to ask the user for credentials it already knows. -->
    <div
      v-if="store.server"
      class="mt-4 flex shrink-0 flex-wrap items-center gap-3 rounded-2xl border border-red-200/70 bg-red-50/70 px-4 py-3 dark:border-red-500/25 dark:bg-red-500/10"
    >
      <span class="text-[11px] font-semibold tracking-wide text-red-400 uppercase">Server</span>
      <span class="min-w-0 flex-1 truncate font-mono text-sm text-red-700 dark:text-red-300">
        {{ store.server.host }}:{{ store.server.port }} · {{ store.server.user }} ·
        {{ store.server.hasPassword ? 'password set' : 'no password' }}
        <!-- Which data this is, stated beside the connection: "New database"
             lands in whichever profile is active, and that has to be obvious
             before the button is clicked, not after. -->
        <template v-if="profilesStore.active"> · {{ profilesStore.active.name }} </template>
      </span>
      <button
        type="button"
        class="flex shrink-0 items-center gap-2 rounded-full border border-red-200 bg-white/80 px-3.5 py-2 text-sm font-semibold text-red-700 transition hover:bg-white dark:border-red-500/30 dark:bg-neutral-900/60 dark:text-red-300"
        :title="store.server.dsn"
        @click="copyDsn"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="h-4 w-4">
          <rect x="9" y="9" width="11" height="11" rx="2" />
          <path stroke-linecap="round" d="M5 15V5a2 2 0 0 1 2-2h8" />
        </svg>
        {{ copiedDsn ? 'Copied' : 'Copy DSN' }}
      </button>
    </div>

    <!-- A stopped MariaDB isn't an error the user made, so it gets an
         explanation and a way forward rather than a raw client message. -->
    <div
      v-if="store.serverDown"
      class="mt-4 shrink-0 rounded-2xl border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-900 dark:border-amber-500/25 dark:bg-amber-500/10 dark:text-amber-200"
    >
      MariaDB isn't running, so there's nothing to list yet. Start it from
      <RouterLink to="/" class="font-semibold underline">Services</RouterLink>, then
      <button type="button" class="font-semibold underline" @click="store.fetchAll">retry</button>.
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
        class="shrink-0 font-semibold text-red-600 underline dark:text-red-400"
        @click="store.openDumpsFolder"
      >
        Show folder
      </button>
    </p>

    <NewDatabaseModal v-if="showNewDatabaseModal" @close="showNewDatabaseModal = false" />
    <ImportSqlModal v-if="importFile" :file="importFile" @close="importFile = null" />

    <BusyOverlay :show="busyLabel !== ''" :label="busyLabel" :detail="busyDetail" />

    <!-- Search, import and the totals sit above the table so the table itself
         can take the rest of the height and scroll inside its own frame. -->
    <div
      v-if="!store.serverDown && store.databases.length > 0"
      class="mt-5 flex shrink-0 flex-wrap items-center gap-3"
    >
      <SearchInput v-model="search" placeholder="Search databases" class="w-full max-w-md" />

      <button
        type="button"
        class="flex shrink-0 items-center gap-2 rounded-full border border-neutral-200 bg-white/70 px-4 py-2.5 text-sm font-semibold text-neutral-700 transition hover:bg-white dark:border-neutral-700 dark:bg-neutral-900/60 dark:text-neutral-200 dark:hover:bg-neutral-800"
        @click="pickSqlFile"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="h-4 w-4">
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            d="M12 20V9m0 0-4 4m4-4 4 4M5 5h14"
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
      No databases yet — create one, or
      <button type="button" class="font-semibold text-red-600 underline" @click="pickSqlFile">
        import a .sql dump</button
      >.
    </div>

    <div
      v-else-if="filteredDatabases.length === 0 && store.databases.length > 0"
      class="mt-8 shrink-0 text-center text-sm text-neutral-500"
    >
      No databases match "{{ search }}".
    </div>

    <div
      v-else-if="filteredDatabases.length > 0"
      class="mt-5 flex min-h-0 flex-1 flex-col overflow-hidden rounded-2xl border border-neutral-200 bg-white dark:border-neutral-800 dark:bg-neutral-900/60"
    >
      <!-- Column headings stay put; only the rows below them move. -->
      <div
        class="flex shrink-0 items-center gap-3 border-b border-neutral-200 bg-neutral-50/80 px-5 py-3 text-[11px] font-semibold tracking-wide text-neutral-400 uppercase dark:border-neutral-800 dark:bg-neutral-900/40"
      >
        <span class="flex-1">Database</span>
        <span class="w-20 shrink-0 text-right">Tables</span>
        <span class="w-24 shrink-0 text-right">Size</span>
        <span class="w-40 shrink-0 pl-6">Used by</span>
        <span class="w-52 shrink-0 text-right">Actions</span>
      </div>

      <div class="min-h-0 flex-1 overflow-y-auto">
        <div
          v-for="db in filteredDatabases"
          :key="db.name"
          class="flex items-center gap-3 border-b border-neutral-200/70 px-5 py-3.5 transition last:border-b-0 hover:bg-neutral-50 dark:border-neutral-800/70 dark:hover:bg-neutral-800/30"
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
          <span class="w-40 shrink-0 truncate pl-6 font-mono text-xs">
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
                  d="M12 4v11m0 0-4-4m4 4 4-4M5 19h14"
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
