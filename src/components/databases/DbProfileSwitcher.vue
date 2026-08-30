<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useDbProfilesStore } from '@/stores/dbProfiles'
import { ENGINE_LABEL, SOURCE_LABEL } from '@/types/dbProfile'
import type { DbProfileStatus } from '@/types/dbProfile'
import AddDbProfileModal from './AddDbProfileModal.vue'

const store = useDbProfilesStore()

const open = ref(false)
const showAddModal = ref(false)

const activeLabel = computed(() => store.active?.name ?? 'No profile')

/** Ordered like the switcher should read: active first, then most recently
 *  used, with never-used profiles last. */
const ordered = computed(() =>
  [...store.profiles].sort((a, b) => {
    if (a.active !== b.active) return a.active ? -1 : 1
    return (b.lastUsedAt ?? 0) - (a.lastUsedAt ?? 0)
  }),
)

function describe(profile: DbProfileStatus) {
  const parts = [ENGINE_LABEL[profile.engine]]
  if (profile.version) parts.push(profile.version)
  if (profile.source !== 'custom') parts.push(SOURCE_LABEL[profile.source])
  return parts.join(' · ')
}

async function choose(profile: DbProfileStatus) {
  if (profile.active || !profile.binaryAvailable) return
  open.value = false
  await store.switchTo(profile.id)
}

function openAddModal() {
  open.value = false
  showAddModal.value = true
  store.detect()
}

onMounted(store.fetchAll)
</script>

<template>
  <div class="relative">
    <button
      type="button"
      class="flex h-9 items-center gap-2 rounded-full border border-neutral-200 bg-white px-3.5 text-sm font-semibold text-neutral-700 transition hover:border-neutral-300 hover:text-neutral-900 disabled:opacity-50 dark:border-neutral-700 dark:bg-neutral-800/60 dark:text-neutral-200 dark:hover:text-neutral-50"
      :disabled="store.switchingId !== null"
      @click="open = !open"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="h-4 w-4">
        <ellipse cx="12" cy="6" rx="8" ry="3" />
        <path stroke-linecap="round" d="M4 6v6c0 1.7 3.6 3 8 3s8-1.3 8-3V6M4 12v6c0 1.7 3.6 3 8 3s8-1.3 8-3v-6" />
      </svg>
      <span class="max-w-[12rem] truncate">
        {{ store.switchingId ? 'Switching…' : activeLabel }}
      </span>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" class="h-3.5 w-3.5">
        <path stroke-linecap="round" stroke-linejoin="round" d="m6 9 6 6 6-6" />
      </svg>
    </button>

    <!-- Click-away layer, so the menu closes like a real dropdown. -->
    <div v-if="open" class="fixed inset-0 z-10" @click="open = false" />

    <div
      v-if="open"
      class="absolute right-0 z-20 mt-2 w-80 overflow-hidden rounded-2xl border border-neutral-200 bg-white shadow-xl dark:border-neutral-700 dark:bg-neutral-900"
    >
      <p class="px-4 pt-3 pb-1 text-[11px] font-semibold tracking-wide text-neutral-400 uppercase">
        Data directory
      </p>

      <button
        v-for="profile in ordered"
        :key="profile.id"
        type="button"
        class="flex w-full items-start gap-3 px-4 py-2.5 text-left transition disabled:cursor-not-allowed hover:bg-neutral-50 dark:hover:bg-neutral-800/60"
        :disabled="!profile.binaryAvailable && !profile.active"
        :title="profile.datadirPath"
        @click="choose(profile)"
      >
        <span
          class="mt-1.5 h-2 w-2 shrink-0 rounded-full"
          :class="profile.active ? 'bg-red-500' : 'bg-neutral-300 dark:bg-neutral-600'"
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
          <span class="block truncate text-xs text-neutral-500">{{ describe(profile) }}</span>
          <!-- Said here rather than on failure: a profile with no matching
               binary can't be switched to, and the reason is fixable. -->
          <span
            v-if="!profile.binaryAvailable"
            class="mt-0.5 block text-xs text-amber-600 dark:text-amber-400"
          >
            No matching {{ ENGINE_LABEL[profile.engine] }} build installed
          </span>
        </span>
        <span v-if="profile.active" class="mt-0.5 shrink-0 text-[11px] font-semibold text-red-500">
          Active
        </span>
      </button>

      <button
        type="button"
        class="flex w-full items-center gap-2 border-t border-neutral-200 px-4 py-3 text-left text-sm font-semibold text-red-600 transition hover:bg-neutral-50 dark:border-neutral-700 dark:text-red-400 dark:hover:bg-neutral-800/60"
        @click="openAddModal"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" class="h-4 w-4">
          <path stroke-linecap="round" d="M12 5v14M5 12h14" />
        </svg>
        Add data directory
      </button>
    </div>

    <AddDbProfileModal v-if="showAddModal" @close="showAddModal = false" />
  </div>
</template>
