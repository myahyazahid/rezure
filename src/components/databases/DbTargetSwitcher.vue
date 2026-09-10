<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useDbProfilesStore } from '@/stores/dbProfiles'
import { useDbConnectionsStore } from '@/stores/dbConnections'
import { useServicesStore } from '@/stores/services'
import { useDatabasesStore } from '@/stores/databases'
import { ENGINE_LABEL, SOURCE_LABEL } from '@/types/dbProfile'
import type { DbProfileStatus } from '@/types/dbProfile'
import type { DbConnectionStatus } from '@/types/dbConnection'
import AddDbProfileModal from './AddDbProfileModal.vue'
import AddConnectionModal from './AddConnectionModal.vue'
import UnlockConnectionModal from './UnlockConnectionModal.vue'

const profiles = useDbProfilesStore()
const connections = useDbConnectionsStore()
const servicesStore = useServicesStore()
const databasesStore = useDatabasesStore()

const open = ref(false)
const showAddProfile = ref(false)
const showAddConnection = ref(false)
/** The connection waiting on a password before it can be selected. */
const unlocking = ref<DbConnectionStatus | null>(null)

/** What the button says: the remote connection when one is selected, the
 *  local profile otherwise. Both can't be active at once — selecting either
 *  clears the other on the Rust side. */
const activeLabel = computed(
  () => connections.active?.name ?? profiles.active?.name ?? 'No profile',
)

/** Ordered like the switcher should read: active first, then most recently
 *  used, with never-used entries last. */
const orderedProfiles = computed(() =>
  [...profiles.profiles].sort((a, b) => {
    if (a.active !== b.active) return a.active ? -1 : 1
    return (b.lastUsedAt ?? 0) - (a.lastUsedAt ?? 0)
  }),
)

const orderedConnections = computed(() =>
  [...connections.connections].sort((a, b) => {
    if (a.active !== b.active) return a.active ? -1 : 1
    return (b.lastUsedAt ?? 0) - (a.lastUsedAt ?? 0)
  }),
)

function describeProfile(profile: DbProfileStatus) {
  const parts = [ENGINE_LABEL[profile.engine]]
  if (profile.version) parts.push(profile.version)
  if (profile.source !== 'custom') parts.push(SOURCE_LABEL[profile.source])
  return parts.join(' · ')
}

function describeConnection(connection: DbConnectionStatus) {
  const endpoint = `${connection.user}@${connection.host}:${connection.port}`
  // The SSH host is the one a user recognises: every tunnelled connection's
  // database host reads as 127.0.0.1, which identifies nothing on its own.
  return connection.ssh ? `${endpoint} via ${connection.ssh.host}` : endpoint
}

/**
 * Switching a profile points the server at a different data directory,
 * which means stopping it — so both the running-service count in the sidebar
 * and the schema list on this page describe the old server the moment it's
 * done. Nothing polls either of them, so they're re-read here rather than
 * left to go stale until the user happens to navigate away and back.
 */
async function chooseProfile(profile: DbProfileStatus) {
  if (!profile.binaryAvailable && !profile.active) return
  open.value = false
  // A local profile is already active but the page is reading a remote
  // connection: there's no datadir switch to do, only a target change.
  if (profile.active) {
    if (!connections.active) return
    await connections.useLocal()
    await databasesStore.fetchAll()
    return
  }
  await profiles.switchTo(profile.id)
  // The Rust side drops the remote target on a profile switch; mirror that
  // here so no connection row is left looking selected.
  connections.clearActiveLocally()
  await Promise.all([servicesStore.fetchAll(), databasesStore.fetchAll()])
}

/**
 * Selecting a connection stops and starts nothing — the local server keeps
 * running on its own datadir — so only the schema list needs re-reading.
 */
async function chooseConnection(connection: DbConnectionStatus) {
  if (connection.active || !connection.clientAvailable) return
  if (!connection.hasPassword) {
    open.value = false
    unlocking.value = connection
    return
  }
  open.value = false
  await connections.use(connection.id)
  await databasesStore.fetchAll()
}

/** Runs once the password has been accepted, continuing the selection the
 *  user already asked for. */
async function afterUnlock() {
  const connection = unlocking.value
  if (!connection) return
  await connections.use(connection.id)
  await databasesStore.fetchAll()
}

async function removeConnection(connection: DbConnectionStatus) {
  const wasActive = connection.active
  await connections.remove(connection.id)
  // Removing the active connection drops the page back to the local server.
  if (wasActive) await databasesStore.fetchAll()
}

function openAddProfile() {
  open.value = false
  showAddProfile.value = true
  profiles.detect()
}

function openAddConnection() {
  open.value = false
  connections.testResult = null
  connections.testError = null
  connections.error = null
  showAddConnection.value = true
}

const SECTION_CLASS =
  'px-4 pt-3 pb-1 text-[11px] font-semibold tracking-wide text-neutral-400 uppercase'
const ROW_CLASS =
  'flex w-full items-start gap-3 px-4 py-2.5 text-left transition disabled:cursor-not-allowed hover:bg-neutral-50 dark:hover:bg-neutral-800/60'

onMounted(() => {
  profiles.fetchAll()
  connections.fetchAll()
})
</script>

<template>
  <div class="relative">
    <button
      type="button"
      class="flex h-9 items-center gap-2 rounded-full border border-neutral-200 bg-white px-3.5 text-sm font-semibold text-neutral-700 transition hover:border-neutral-300 hover:text-neutral-900 disabled:opacity-50 dark:border-neutral-700 dark:bg-neutral-800/60 dark:text-neutral-200 dark:hover:text-neutral-50"
      :disabled="profiles.switchingId !== null || connections.switchingId !== null"
      @click="open = !open"
    >
      <!-- A remote target gets its own glyph: which server the page is
           reading is the one thing this button must never be ambiguous
           about. -->
      <svg
        v-if="connections.active"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        class="h-4 w-4"
      >
        <rect x="3" y="4" width="18" height="7" rx="2" />
        <rect x="3" y="13" width="18" height="7" rx="2" />
        <path stroke-linecap="round" d="M7 7.5h.01M7 16.5h.01" />
      </svg>
      <svg
        v-else
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        class="h-4 w-4"
      >
        <ellipse cx="12" cy="6" rx="8" ry="3" />
        <path
          stroke-linecap="round"
          d="M4 6v6c0 1.7 3.6 3 8 3s8-1.3 8-3V6M4 12v6c0 1.7 3.6 3 8 3s8-1.3 8-3v-6"
        />
      </svg>
      <span class="max-w-[12rem] truncate">
        {{ profiles.switchingId || connections.switchingId ? 'Switching…' : activeLabel }}
      </span>
      <svg
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2.5"
        class="h-3.5 w-3.5"
      >
        <path stroke-linecap="round" stroke-linejoin="round" d="m6 9 6 6 6-6" />
      </svg>
    </button>

    <!-- Click-away layer, so the menu closes like a real dropdown. -->
    <div v-if="open" class="fixed inset-0 z-10" @click="open = false" />

    <div
      v-if="open"
      class="absolute right-0 z-20 mt-2 max-h-[70vh] w-80 overflow-y-auto rounded-2xl border border-neutral-200 bg-white shadow-xl dark:border-neutral-700 dark:bg-neutral-900"
    >
      <p :class="SECTION_CLASS">Data directory</p>

      <button
        v-for="profile in orderedProfiles"
        :key="profile.id"
        type="button"
        :class="ROW_CLASS"
        :disabled="!profile.binaryAvailable && !profile.active"
        :title="profile.datadirPath"
        @click="chooseProfile(profile)"
      >
        <span
          class="mt-1.5 h-2 w-2 shrink-0 rounded-full"
          :class="
            profile.active && !connections.active
              ? 'bg-red-500'
              : 'bg-neutral-300 dark:bg-neutral-600'
          "
        />
        <span class="min-w-0 flex-1">
          <span
            class="block truncate text-sm font-semibold"
            :class="
              profile.binaryAvailable || profile.active
                ? 'text-neutral-900 dark:text-neutral-100'
                : 'text-neutral-400 dark:text-neutral-500'
            "
          >
            {{ profile.name }}
          </span>
          <span class="block truncate text-xs text-neutral-500">{{
            describeProfile(profile)
          }}</span>
          <!-- Said here rather than on failure: a profile with no matching
               binary can't be switched to, and the reason is fixable. -->
          <span
            v-if="!profile.binaryAvailable"
            class="mt-0.5 block text-xs text-amber-600 dark:text-amber-400"
          >
            No matching {{ ENGINE_LABEL[profile.engine] }} build installed
          </span>
        </span>
        <span
          v-if="profile.active && !connections.active"
          class="mt-0.5 shrink-0 text-[11px] font-semibold text-red-500"
        >
          Active
        </span>
      </button>

      <button
        type="button"
        class="flex w-full items-center gap-2 px-4 py-2.5 text-left text-sm font-semibold text-red-600 transition hover:bg-neutral-50 dark:text-red-400 dark:hover:bg-neutral-800/60"
        @click="openAddProfile"
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
        Add data directory
      </button>

      <div class="border-t border-neutral-200 dark:border-neutral-700">
        <p :class="SECTION_CLASS">Connections</p>

        <p v-if="orderedConnections.length === 0" class="px-4 pb-2 text-xs text-neutral-500">
          Servers Rezure doesn't run — staging, a VPS. Rezure lists, exports and imports; it never
          starts or stops them.
        </p>

        <div v-for="connection in orderedConnections" :key="connection.id" class="group relative">
          <button
            type="button"
            :class="[ROW_CLASS, 'pr-10']"
            :disabled="!connection.clientAvailable"
            :title="describeConnection(connection)"
            @click="chooseConnection(connection)"
          >
            <span
              class="mt-1.5 h-2 w-2 shrink-0 rounded-full"
              :class="connection.active ? 'bg-red-500' : 'bg-neutral-300 dark:bg-neutral-600'"
            />
            <span class="min-w-0 flex-1">
              <span
                class="block truncate text-sm font-semibold"
                :class="
                  connection.clientAvailable
                    ? 'text-neutral-900 dark:text-neutral-100'
                    : 'text-neutral-400 dark:text-neutral-500'
                "
              >
                {{ connection.name }}
              </span>
              <span class="block truncate font-mono text-xs text-neutral-500">
                {{ describeConnection(connection) }}
              </span>
              <span class="mt-0.5 flex flex-wrap gap-1.5">
                <span
                  v-if="connection.readOnly"
                  class="rounded-full bg-neutral-100 px-1.5 py-0.5 text-[10px] font-semibold text-neutral-500 dark:bg-neutral-800 dark:text-neutral-400"
                >
                  Read-only
                </span>
                <!-- A connection with no password known can still be picked;
                     it asks for one first rather than failing on the query. -->
                <span
                  v-if="!connection.hasPassword"
                  class="rounded-full bg-amber-50 px-1.5 py-0.5 text-[10px] font-semibold text-amber-700 dark:bg-amber-500/10 dark:text-amber-300"
                >
                  Needs password
                </span>
              </span>
              <span
                v-if="!connection.clientAvailable"
                class="mt-0.5 block text-xs text-amber-600 dark:text-amber-400"
              >
                No {{ ENGINE_LABEL[connection.engine] }} client installed to reach it
              </span>
            </span>
            <span
              v-if="connection.active"
              class="mt-0.5 shrink-0 text-[11px] font-semibold text-red-500"
            >
              Active
            </span>
          </button>

          <!-- Deleting is the only way to get rid of a connection, so it has
               to be reachable — but never the thing a mis-click hits, hence
               hover-only and out of the row's own click target. -->
          <button
            type="button"
            class="absolute top-3 right-3 hidden rounded-lg p-1.5 text-neutral-400 transition group-hover:block hover:bg-neutral-100 hover:text-red-600 dark:hover:bg-neutral-800"
            :title="`Remove ${connection.name}`"
            @click.stop="removeConnection(connection)"
          >
            <svg
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
              class="h-4 w-4"
            >
              <path stroke-linecap="round" d="M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3" />
            </svg>
          </button>
        </div>

        <button
          type="button"
          class="flex w-full items-center gap-2 px-4 py-3 text-left text-sm font-semibold text-red-600 transition hover:bg-neutral-50 dark:text-red-400 dark:hover:bg-neutral-800/60"
          @click="openAddConnection"
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
          Add connection
        </button>
      </div>
    </div>

    <AddDbProfileModal v-if="showAddProfile" @close="showAddProfile = false" />
    <AddConnectionModal v-if="showAddConnection" @close="showAddConnection = false" />
    <UnlockConnectionModal
      v-if="unlocking"
      :connection="unlocking"
      @unlocked="afterUnlock"
      @close="unlocking = null"
    />
  </div>
</template>
