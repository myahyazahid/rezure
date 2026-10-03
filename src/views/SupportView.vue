<script setup lang="ts">
import { computed, onActivated } from 'vue'
import { open as openFileDialog } from '@tauri-apps/plugin-dialog'
import { useSupportStore } from '@/stores/support'
import { useLogsStore } from '@/stores/logs'
import type { TicketCategory, TicketStatus } from '@/types/support'

const store = useSupportStore()
const logsStore = useLogsStore()

onActivated(() => {
  store.fetchHistory()
})

const CATEGORIES: { value: TicketCategory; label: string }[] = [
  { value: 'bug', label: 'Bug Report' },
  { value: 'feature_request', label: 'Feature Request' },
  { value: 'general', label: 'General Feedback' },
]

const STATUS_LABEL: Record<TicketStatus, string> = {
  open: 'Open',
  in_progress: 'In Progress',
  resolved: 'Resolved',
}

const canSubmit = computed(
  () => store.title.trim().length > 0 && store.description.trim().length > 0 && !store.submitting,
)

async function pickAttachments() {
  const picked = await openFileDialog({
    multiple: true,
    filters: [
      {
        name: 'Attachments',
        extensions: ['png', 'jpg', 'jpeg', 'gif', 'webp', 'txt', 'log', 'zip'],
      },
    ],
  })
  if (!picked) return
  const paths = Array.isArray(picked) ? picked : [picked]
  for (const path of paths) {
    await store.addAttachment(path)
  }
}

/** Pulls the most recent lines from the existing log viewer's buffer — there
 * is no on-disk log file to read, so this reuses what's already in memory. */
function attachLatestLog() {
  const recent = logsStore.entries.slice(0, 200).slice().reverse()
  const text = recent.map((e) => `[${e.time}] [${e.service}] [${e.level}] ${e.message}`).join('\n')
  store.setLogText(text || null)
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}
</script>

<template>
  <section>
    <h1 class="text-2xl font-semibold text-neutral-900 dark:text-neutral-100">Feedback</h1>
    <p class="mt-1 text-sm text-neutral-500">
      Report a bug, request a feature, or send general feedback.
    </p>

    <div
      v-if="store.submitted"
      class="mt-6 rounded-2xl border border-green-300/60 bg-green-100/50 p-5 dark:border-green-900 dark:bg-green-950/40"
    >
      <p class="font-semibold text-green-800 dark:text-green-300">Ticket sent</p>
      <p class="mt-1 text-sm text-green-700 dark:text-green-400">
        Thanks — we've received your report.
      </p>
      <button
        type="button"
        class="mt-4 rounded-full bg-green-700 px-5 py-2 text-sm font-semibold text-white transition hover:bg-green-600"
        @click="store.startNewTicket()"
      >
        Send another
      </button>
    </div>

    <template v-else>
      <div class="glass mt-6 rounded-2xl p-5">
        <div class="grid gap-4 sm:grid-cols-2">
          <div>
            <label class="block text-sm font-semibold text-neutral-700 dark:text-neutral-200">
              Category
            </label>
            <select
              :value="store.category"
              class="glass-inset mt-1.5 w-full rounded-lg px-3 py-2 text-sm dark:text-neutral-100"
              @change="
                store.setCategory(($event.target as HTMLSelectElement).value as TicketCategory)
              "
            >
              <option v-for="c in CATEGORIES" :key="c.value" :value="c.value">
                {{ c.label }}
              </option>
            </select>
          </div>
          <div>
            <label class="block text-sm font-semibold text-neutral-700 dark:text-neutral-200">
              Title
            </label>
            <input
              :value="store.title"
              type="text"
              maxlength="150"
              placeholder="Short summary"
              class="glass-inset mt-1.5 w-full rounded-lg px-3 py-2 text-sm dark:text-neutral-100"
              @input="store.setTitle(($event.target as HTMLInputElement).value)"
            />
          </div>
        </div>

        <div class="mt-4">
          <label class="block text-sm font-semibold text-neutral-700 dark:text-neutral-200">
            Description
          </label>
          <textarea
            :value="store.description"
            rows="5"
            maxlength="5000"
            placeholder="What happened, what you expected, steps to reproduce…"
            class="glass-inset mt-1.5 w-full rounded-lg px-3 py-2 text-sm dark:text-neutral-100"
            @input="store.setDescription(($event.target as HTMLTextAreaElement).value)"
          />
        </div>

        <div class="mt-4 flex items-center gap-2">
          <input
            id="include-system-info"
            type="checkbox"
            :checked="store.includeSystemInfo"
            class="h-4 w-4 rounded border-neutral-900/20 dark:border-white/20"
            @change="store.setIncludeSystemInfo(($event.target as HTMLInputElement).checked)"
          />
          <label for="include-system-info" class="text-sm text-neutral-600 dark:text-neutral-300">
            Include Rezure version &amp; OS info
          </label>
        </div>
      </div>

      <div class="glass mt-4 rounded-2xl p-5">
        <div class="flex items-center justify-between">
          <p class="text-sm font-semibold text-neutral-900 dark:text-neutral-100">Attachments</p>
          <div class="flex gap-2">
            <button
              type="button"
              class="glass-btn rounded-full px-3 py-1.5 text-xs font-semibold text-neutral-600 transition hover:text-accent-600 dark:text-neutral-300"
              :disabled="!store.logText && store.attachmentSlotsLeft <= 0"
              @click="attachLatestLog"
            >
              Attach latest log
            </button>
            <button
              type="button"
              class="glass-btn rounded-full px-3 py-1.5 text-xs font-semibold text-neutral-600 transition hover:text-accent-600 dark:text-neutral-300"
              :disabled="store.attachmentSlotsLeft <= 0"
              @click="pickAttachments"
            >
              Browse…
            </button>
          </div>
        </div>

        <p v-if="store.logText" class="mt-2 text-xs text-neutral-500">
          Latest log lines will be attached as "latest-log.txt".
          <button
            type="button"
            class="text-red-600 hover:underline"
            @click="store.setLogText(null)"
          >
            Remove
          </button>
        </p>

        <ul v-if="store.attachments.length" class="mt-3 space-y-1.5">
          <li
            v-for="a in store.attachments"
            :key="a.path"
            class="glass-inset flex items-center justify-between rounded-lg px-3 py-1.5 text-xs"
          >
            <span class="truncate text-neutral-700 dark:text-neutral-200">{{ a.name }}</span>
            <span class="ml-2 flex shrink-0 items-center gap-2">
              <span class="text-neutral-400">{{ formatSize(a.sizeBytes) }}</span>
              <button
                type="button"
                class="text-neutral-400 hover:text-red-600"
                @click="store.removeAttachment(a.path)"
              >
                Remove
              </button>
            </span>
          </li>
        </ul>
        <p v-else class="mt-3 text-xs text-neutral-500">
          Up to 5 files — screenshots, .txt/.log, or a .zip. Max 10MB each.
        </p>

        <p v-if="store.attachmentError" class="mt-2 text-xs text-red-600 dark:text-red-400">
          {{ store.attachmentError }}
        </p>
      </div>

      <p v-if="store.submitError" class="mt-4 text-sm text-red-600 dark:text-red-400">
        {{ store.submitError }}
      </p>

      <div class="mt-4 flex justify-end gap-2">
        <button
          v-if="store.submitError"
          type="button"
          class="glass-btn rounded-full px-5 py-2 text-sm font-semibold text-neutral-600 dark:text-neutral-300"
          :disabled="store.submitting"
          @click="store.submit()"
        >
          Retry
        </button>
        <button
          v-else
          type="button"
          class="glass-accent flex items-center gap-2 rounded-full px-4 py-2 text-sm font-semibold transition disabled:opacity-50"
          :disabled="!canSubmit"
          @click="store.submit()"
        >
          <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            aria-hidden="true"
            class="h-4 w-4"
          >
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              d="M21 3 10 14M21 3l-7 18-4-7-7-4 18-7Z"
            />
          </svg>
          {{ store.submitting ? 'Sending…' : 'Send ticket' }}
        </button>
      </div>
    </template>

    <div class="mt-8">
      <h2 class="text-sm font-semibold text-neutral-900 dark:text-neutral-100">Your reports</h2>
      <p v-if="store.historyError" class="mt-1 text-xs text-neutral-500">
        Couldn't load your ticket history right now.
      </p>
      <p
        v-else-if="!store.loadingHistory && store.history.length === 0"
        class="mt-1 text-xs text-neutral-500"
      >
        Nothing sent yet.
      </p>
      <ul v-else class="glass mt-3 divide-y divide-neutral-900/8 rounded-2xl dark:divide-white/8">
        <li
          v-for="(item, i) in store.history"
          :key="i"
          class="flex items-center justify-between gap-4 p-4 text-sm"
        >
          <div class="min-w-0">
            <p class="truncate font-semibold text-neutral-900 dark:text-neutral-100">
              {{ item.title }}
            </p>
            <p class="mt-0.5 text-xs text-neutral-500">
              {{ CATEGORIES.find((c) => c.value === item.category)?.label ?? item.category }} ·
              {{ new Date(item.createdAt).toLocaleString() }}
            </p>
          </div>
          <span
            class="shrink-0 rounded-full px-2.5 py-1 text-xs font-semibold"
            :class="
              item.status === 'resolved'
                ? 'border border-transparent bg-green-500/15 text-green-700 dark:bg-green-900/40 dark:text-green-300'
                : item.status === 'in_progress'
                  ? 'border border-transparent bg-amber-500/15 text-amber-700 dark:bg-amber-900/40 dark:text-amber-300'
                  : 'glass-inset text-neutral-600 dark:text-neutral-300'
            "
          >
            {{ STATUS_LABEL[item.status] }}
          </span>
        </li>
      </ul>
    </div>
  </section>
</template>
