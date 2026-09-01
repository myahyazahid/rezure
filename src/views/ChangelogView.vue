<script setup lang="ts">
import { computed, onActivated } from 'vue'
import { marked } from 'marked'
import DOMPurify from 'dompurify'
import { useChangelogStore } from '@/stores/changelog'

const store = useChangelogStore()

onActivated(async () => {
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
</script>

<template>
  <section>
    <h1 class="text-2xl font-semibold text-neutral-900 dark:text-neutral-100">Changelog</h1>
    <p class="mt-1 text-sm text-neutral-500">What's new in Rezure, release by release.</p>

    <p v-if="store.loading && !hasEntries" class="mt-6 text-sm text-neutral-500">Loading…</p>

    <p
      v-else-if="!hasEntries"
      class="mt-6 rounded-2xl border border-neutral-200 bg-white p-5 text-sm text-neutral-500 dark:border-neutral-800 dark:bg-neutral-900/60"
    >
      No changelog entries yet.
    </p>

    <div v-else class="mt-6 space-y-4">
      <article
        v-for="entry in store.entries"
        :key="entry.version"
        class="rounded-2xl border border-neutral-200 bg-white p-5 dark:border-neutral-800 dark:bg-neutral-900/60"
      >
        <div class="flex flex-wrap items-baseline justify-between gap-2">
          <h2 class="text-base font-semibold text-neutral-900 dark:text-neutral-100">
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
          class="changelog-body mt-3 text-sm text-neutral-700 dark:text-neutral-300"
          v-html="renderBody(entry.body)"
        />
      </article>
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
