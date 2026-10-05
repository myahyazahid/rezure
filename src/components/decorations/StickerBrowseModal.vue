<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useDecorationsStore } from '@/stores/decorations'
import { useStickerLibraryStore } from '@/stores/stickerLibrary'
import type { DownloadState } from '@/types/stickerLibrary'

/**
 * Decorations → Browse: the sticker catalog from Rezure's server, with a
 * button to download each one into the palette.
 *
 * This only lists, previews and asks. Whether a download is kept is decided in
 * Rust (checksum, format, size); a sticker shows as downloaded here only once
 * the backend has saved it.
 */
const emit = defineEmits<{ close: [] }>()

const library = useStickerLibraryStore()
const decorations = useDecorationsStore()

/** One tile, from either the catalog or — for a sticker the server no longer
 *  offers — the downloads. */
interface Card {
  id: string
  name: string
  category: string
  size: number
  state: DownloadState
  previewUrl: string
}

const query = ref('')
/** `all`, `downloaded`, or a category slug. */
const filter = ref('all')

const cards = computed<Card[]>(() => {
  const fromCatalog = library.catalog.map((c) => ({
    id: c.id,
    name: c.name,
    category: c.category,
    size: c.size,
    state: library.stateOf(c),
    // A downloaded sticker previews from disk: instant, and works offline.
    previewUrl: library.urlFor(c.id) || c.previewUrl,
  }))
  const offered = new Set(library.catalog.map((c) => c.id))
  const withdrawn = library.saved
    .filter((s) => !offered.has(s.id))
    .map((s) => ({
      id: s.id,
      name: s.name,
      category: s.category,
      size: s.size,
      state: 'saved' as const,
      previewUrl: s.dataUrl,
    }))

  const q = query.value.trim().toLowerCase()
  return [...fromCatalog, ...withdrawn].filter((card) => {
    const inFilter =
      filter.value === 'all' ||
      (filter.value === 'downloaded' ? card.state !== 'new' : card.category === filter.value)
    return inFilter && (q === '' || `${card.name} ${card.category}`.toLowerCase().includes(q))
  })
})

const chips = computed(() => [
  { value: 'all', label: 'All' },
  ...library.categories.map((c) => ({ value: c, label: c.replace(/-/g, ' ') })),
  { value: 'downloaded', label: 'Downloaded' },
])

const hasAnything = computed(() => library.catalog.length > 0 || library.saved.length > 0)

function kb(bytes: number): string {
  return `${(bytes / 1024).toFixed(1)} KB`
}

// A preview that fails to load (offline, a withdrawn file) shows a label
// instead of the browser's broken-image icon.
const failedPreviews = ref<string[]>([])

// Removing a sticker that is on the window also takes it off the window, so
// that asks for a second click; one that isn't placed anywhere just goes.
const confirmingId = ref<string | null>(null)
let confirmTimer: ReturnType<typeof setTimeout> | undefined

async function onRemove(card: Card) {
  const placed = decorations.placedCount(card.id)
  if (placed > 0 && confirmingId.value !== card.id) {
    confirmingId.value = card.id
    clearTimeout(confirmTimer)
    confirmTimer = setTimeout(() => (confirmingId.value = null), 3500)
    return
  }
  clearTimeout(confirmTimer)
  confirmingId.value = null
  await decorations.removeSaved(card.id)
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') emit('close')
}

onMounted(() => {
  window.addEventListener('keydown', onKeydown)
  // Every opening asks again: unchanged is an empty 304, changed is news.
  library.fetchCatalog()
})
onUnmounted(() => {
  window.removeEventListener('keydown', onKeydown)
  clearTimeout(confirmTimer)
})

const CHIP_BASE = 'rounded-full px-2.5 py-0.5 text-[11px] font-semibold capitalize transition'
</script>

<template>
  <div
    class="glass-scrim fixed inset-0 z-50 flex items-center justify-center p-4"
    @click.self="emit('close')"
  >
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="browse-title"
      class="glass-strong flex max-h-[88vh] w-full max-w-3xl flex-col rounded-2xl p-6"
    >
      <div class="flex items-start justify-between gap-4">
        <div>
          <h2 id="browse-title" class="text-lg font-bold text-neutral-900 dark:text-neutral-100">
            Browse stickers
          </h2>
          <p class="mt-1 text-sm text-neutral-500">
            More stickers from Rezure's catalog. Download one and it joins your palette — it keeps
            working offline.
          </p>
        </div>
        <button
          type="button"
          aria-label="Close"
          class="glass-btn flex h-8 w-8 shrink-0 items-center justify-center rounded-full text-neutral-600 transition dark:text-neutral-300"
          @click="emit('close')"
        >
          <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2.5"
            class="h-4 w-4"
          >
            <path stroke-linecap="round" d="M6 6l12 12M18 6L6 18" />
          </svg>
        </button>
      </div>

      <div class="mt-4 flex flex-wrap items-center gap-2">
        <input
          v-model="query"
          type="search"
          placeholder="Search stickers"
          aria-label="Search stickers"
          class="glass-inset min-w-40 flex-1 rounded-xl px-3.5 py-2 text-sm text-neutral-900 outline-none transition focus:border-accent-400/70 dark:text-neutral-100"
        />
        <div class="glass-inset flex flex-wrap gap-0.5 rounded-2xl p-0.5">
          <button
            v-for="chip in chips"
            :key="chip.value"
            type="button"
            :class="[
              CHIP_BASE,
              filter === chip.value
                ? 'glass-raised text-neutral-900 dark:text-neutral-100'
                : 'glass-ghost',
            ]"
            @click="filter = chip.value"
          >
            {{ chip.label }}
          </button>
        </div>
      </div>

      <div
        v-if="library.offline"
        class="mt-3 flex items-center justify-between gap-3 rounded-xl bg-amber-100/50 px-3 py-2 text-sm text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
      >
        <span>
          {{ library.catalogError ?? "Can't reach the sticker catalog" }}.
          {{ library.catalog.length > 0 ? 'Showing what was loaded last time.' : '' }}
        </span>
        <button
          type="button"
          class="glass-btn shrink-0 rounded-full px-3 py-1 text-xs font-semibold transition disabled:opacity-50"
          :disabled="library.loadingCatalog"
          @click="library.fetchCatalog()"
        >
          {{ library.loadingCatalog ? 'Trying…' : 'Try again' }}
        </button>
      </div>

      <p v-if="library.error" class="mt-3 text-sm text-red-600 dark:text-red-400" role="alert">
        {{ library.error }}
      </p>

      <div class="mt-4 min-h-0 flex-1 overflow-y-auto pr-1 [scrollbar-width:thin]">
        <p
          v-if="library.loadingCatalog && !library.catalogLoaded && !hasAnything"
          class="py-10 text-center text-sm text-neutral-500"
        >
          Loading stickers…
        </p>
        <p v-else-if="!hasAnything" class="py-10 text-center text-sm text-neutral-500">
          {{
            library.offline
              ? 'Connect to the internet to see the catalog.'
              : 'No stickers published yet — check back soon.'
          }}
        </p>
        <p v-else-if="cards.length === 0" class="py-10 text-center text-sm text-neutral-500">
          Nothing matches that.
        </p>

        <ul
          v-else
          class="grid grid-cols-[repeat(auto-fill,minmax(8.5rem,1fr))] gap-2.5"
          aria-label="Stickers"
        >
          <li
            v-for="card in cards"
            :key="card.id"
            class="glass-inset flex flex-col gap-2 rounded-xl p-2.5"
          >
            <div
              class="flex aspect-square items-center justify-center rounded-lg bg-white/30 p-3 dark:bg-white/5"
            >
              <img
                v-if="!failedPreviews.includes(card.id)"
                :src="card.previewUrl"
                alt=""
                loading="lazy"
                draggable="false"
                class="max-h-full max-w-full"
                @error="failedPreviews.push(card.id)"
              />
              <span v-else class="text-xs text-neutral-500">No preview</span>
            </div>

            <div class="min-w-0">
              <p
                class="truncate text-sm font-semibold text-neutral-900 dark:text-neutral-100"
                :title="card.name"
              >
                {{ card.name }}
              </p>
              <p class="truncate text-xs text-neutral-500 capitalize">
                {{ card.category.replace(/-/g, ' ') }} · {{ kb(card.size) }}
              </p>
            </div>

            <div class="mt-auto flex flex-col gap-1.5">
              <button
                v-if="card.state !== 'saved'"
                type="button"
                class="glass-accent rounded-full px-3 py-1.5 text-xs font-semibold transition disabled:cursor-wait disabled:opacity-60"
                :disabled="library.isBusy(card.id)"
                :aria-label="`${card.state === 'outdated' ? 'Update' : 'Download'} ${card.name}`"
                @click="library.download(card.id)"
              >
                {{
                  library.isBusy(card.id)
                    ? 'Downloading…'
                    : card.state === 'outdated'
                      ? 'Update'
                      : 'Download'
                }}
              </button>
              <p
                v-else
                class="flex items-center justify-center gap-1 py-1.5 text-xs font-semibold text-emerald-700 dark:text-emerald-300"
              >
                <svg
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="3"
                  class="h-3.5 w-3.5"
                  aria-hidden="true"
                >
                  <path stroke-linecap="round" stroke-linejoin="round" d="M5 12.5l4.5 4.5L19 7" />
                </svg>
                Downloaded
              </p>

              <button
                v-if="card.state !== 'new'"
                type="button"
                class="rounded-full px-2 py-0.5 text-[11px] font-semibold text-neutral-500 transition hover:text-red-600 dark:hover:text-red-400"
                :class="confirmingId === card.id ? 'text-red-600 dark:text-red-400' : ''"
                :aria-label="`Remove ${card.name}`"
                @click="onRemove(card)"
              >
                {{
                  confirmingId === card.id
                    ? `Click again — also removes ${decorations.placedCount(card.id)} on the window`
                    : 'Remove'
                }}
              </button>
            </div>
          </li>
        </ul>
      </div>

      <p class="mt-4 text-xs text-neutral-500">
        {{ library.saved.length }} downloaded — find them under the
        <span class="font-semibold">Downloaded</span> tab of the sticker palette.
      </p>
    </div>
  </div>
</template>
