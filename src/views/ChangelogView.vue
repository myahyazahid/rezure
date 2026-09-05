<script setup lang="ts">
import { computed, onActivated, ref, useTemplateRef } from 'vue'
import { marked } from 'marked'
import DOMPurify from 'dompurify'
import { useChangelogStore } from '@/stores/changelog'

const store = useChangelogStore()

const PAGE_SIZE = 10
const page = ref(1)
const topRef = useTemplateRef<HTMLElement>('top')

onActivated(async () => {
  // Back to the newest on every visit — the reason to open this page is to
  // see what changed, not to resume where the last visit left off.
  page.value = 1
  await store.fetchAll()
  await store.markSeen()
})

function renderBody(markdown: string): string {
  return DOMPurify.sanitize(marked.parse(markdown, { async: false }))
}

function formatDate(iso: string): string {
  return new Date(iso).toLocaleDateString(undefined, {
    year: 'numeric',
    month: 'long',
    day: 'numeric',
  })
}

const hasEntries = computed(() => store.entries.length > 0)

const totalPages = computed(() => Math.max(1, Math.ceil(store.entries.length / PAGE_SIZE)))

// Clamped rather than read raw: a refetch can return fewer entries than the
// page the user is sitting on, which would otherwise render an empty list.
const currentPage = computed(() => Math.min(page.value, totalPages.value))

const pagedEntries = computed(() => {
  const start = (currentPage.value - 1) * PAGE_SIZE
  return store.entries.slice(start, start + PAGE_SIZE)
})

function goToPage(next: number) {
  page.value = Math.min(Math.max(next, 1), totalPages.value)
  // The list is replaced under a scroll position that belongs to the old
  // page, so send the reader back to the first entry of the new one.
  topRef.value?.scrollIntoView({ block: 'start' })
}
</script>

<template>
  <section>
    <h1 ref="top" class="text-2xl font-semibold text-neutral-900 dark:text-neutral-100">
      Changelog
    </h1>
    <p class="mt-1 text-sm text-neutral-500">What's new in Rezure, release by release.</p>

    <p v-if="store.loading && !hasEntries" class="mt-6 text-sm text-neutral-500">Loading…</p>

    <p
      v-else-if="!hasEntries"
      class="mt-6 rounded-2xl border border-neutral-200 bg-white p-5 text-sm text-neutral-500 dark:border-neutral-800 dark:bg-neutral-900/60"
    >
      No changelog entries yet.
    </p>

    <div v-else class="mt-5 space-y-2.5">
      <article
        v-for="entry in pagedEntries"
        :key="entry.version"
        class="rounded-2xl border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900/60"
      >
        <div class="flex flex-wrap items-baseline justify-between gap-2">
          <h2 class="font-semibold text-neutral-900 dark:text-neutral-100">
            {{ entry.title }}
          </h2>
          <div class="flex items-center gap-2 text-xs text-neutral-500">
            <span
              class="rounded-full bg-red-50 px-2 py-0.5 font-mono font-semibold text-red-600 dark:bg-red-500/10 dark:text-red-400"
            >
              v{{ entry.version }}
            </span>
            <span>{{ formatDate(entry.releasedAt) }}</span>
          </div>
        </div>
        <!-- eslint-disable-next-line vue/no-v-html -->
        <div
          class="changelog-body mt-2 text-sm text-neutral-700 dark:text-neutral-300"
          v-html="renderBody(entry.body)"
        />
      </article>
    </div>

    <!-- One page of releases needs no controls. -->
    <div v-if="hasEntries && totalPages > 1" class="mt-4 flex items-center justify-between gap-3">
      <button
        type="button"
        class="rounded-full border border-neutral-200 bg-white/70 px-4 py-1.5 text-sm font-semibold text-neutral-700 transition hover:bg-white disabled:opacity-40 dark:border-neutral-700 dark:bg-neutral-900/60 dark:text-neutral-200 dark:hover:bg-neutral-800"
        :disabled="currentPage === 1"
        @click="goToPage(currentPage - 1)"
      >
        Newer
      </button>
      <span class="text-xs text-neutral-500">Page {{ currentPage }} of {{ totalPages }}</span>
      <button
        type="button"
        class="rounded-full border border-neutral-200 bg-white/70 px-4 py-1.5 text-sm font-semibold text-neutral-700 transition hover:bg-white disabled:opacity-40 dark:border-neutral-700 dark:bg-neutral-900/60 dark:text-neutral-200 dark:hover:bg-neutral-800"
        :disabled="currentPage === totalPages"
        @click="goToPage(currentPage + 1)"
      >
        Older
      </button>
    </div>
  </section>
</template>

<style scoped>
/* No Tailwind Typography plugin in this project — minimal styling for
   maintainer-authored markdown instead of pulling in the plugin for one
   view. */
.changelog-body :deep(h1),
.changelog-body :deep(h2),
.changelog-body :deep(h3) {
  margin-top: 1em;
  margin-bottom: 0.4em;
  font-weight: 600;
  color: inherit;
}
.changelog-body :deep(p) {
  margin: 0.6em 0;
}
/* Markdown's outer margins would otherwise stack on top of the card padding,
   which on a one-line entry is most of the card's height. */
.changelog-body :deep(> :first-child) {
  margin-top: 0;
}
.changelog-body :deep(> :last-child) {
  margin-bottom: 0;
}
.changelog-body :deep(ul),
.changelog-body :deep(ol) {
  margin: 0.6em 0;
  padding-left: 1.4em;
}
.changelog-body :deep(li) {
  margin: 0.2em 0;
}
.changelog-body :deep(li) {
  list-style: revert;
}
.changelog-body :deep(a) {
  color: rgb(220 38 38);
  text-decoration: underline;
}
.changelog-body :deep(code) {
  border-radius: 0.25rem;
  background: rgba(115, 115, 115, 0.15);
  padding: 0.1em 0.35em;
  font-size: 0.85em;
}
.changelog-body :deep(pre) {
  overflow-x: auto;
  border-radius: 0.5rem;
  background: rgba(115, 115, 115, 0.15);
  padding: 0.75em;
}
.changelog-body :deep(pre code) {
  background: none;
  padding: 0;
}
</style>
