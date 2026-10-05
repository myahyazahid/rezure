<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import StickerBrowseModal from '@/components/decorations/StickerBrowseModal.vue'
import {
  MAX_SIZE,
  MAX_STICKERS,
  MIN_SIZE,
  STICKERS,
  STICKER_CATEGORIES,
  savedKind,
  stickerLabel,
  stickerStyle,
  stickerUrl,
  useDecorationsStore,
  type StickerCategory,
} from '@/stores/decorations'
import { useStickerLibraryStore } from '@/stores/stickerLibrary'
import type { Sticker, StickerArt } from '@/types/settings'

const store = useDecorationsStore()
const library = useStickerLibraryStore()

const selectedId = ref<string | null>(null)
const selected = computed(() => store.stickers.find((s) => s.id === selectedId.value) ?? null)
const selectedLabel = computed(() => (selected.value ? stickerLabel(selected.value.kind) : ''))

/** What the preview draws: a downloaded sticker with no image to show (its
 *  file removed behind our back) is left out rather than drawn broken. */
const drawn = computed(() => store.stickers.filter((s) => stickerUrl(s.kind) !== ''))

/** A tile in the palette — built-in art or a download. */
interface PaletteItem {
  kind: StickerArt
  label: string
  url: string
}

type PaletteTab = 'All' | StickerCategory | 'Downloaded'

const showBrowse = ref(false)

const category = ref<PaletteTab>('All')
const categoryTabs = computed<PaletteTab[]>(() => ['All', ...STICKER_CATEGORIES, 'Downloaded'])
const downloaded = computed<PaletteItem[]>(() =>
  library.saved.map((s) => ({ kind: savedKind(s.id), label: s.name, url: s.dataUrl })),
)
const visibleStickers = computed<PaletteItem[]>(() => {
  if (category.value === 'Downloaded') return downloaded.value
  const builtIn: PaletteItem[] = (
    category.value === 'All' ? STICKERS : STICKERS.filter((s) => s.category === category.value)
  ).map((s) => ({ kind: s.kind, label: s.label, url: s.url }))
  return category.value === 'All' ? [...builtIn, ...downloaded.value] : builtIn
})

// The preview has the window's own proportions and layout, so a sticker put
// somewhere in it lands on the same spot of the real window. Tracked live, so
// resizing the window reshapes the preview with it.
const windowSize = ref({ w: window.innerWidth, h: window.innerHeight })
function onResize() {
  windowSize.value = { w: window.innerWidth, h: window.innerHeight }
}
onMounted(() => window.addEventListener('resize', onResize))
onUnmounted(() => window.removeEventListener('resize', onResize))

/** Pixel measurements of the real layout (App.vue, AppTitleBar, AppSidebar)
 *  as percentages of the window, for the miniature. */
const layout = computed(() => {
  const { w, h } = windowSize.value
  const pw = (px: number) => `${(px / w) * 100}%`
  const ph = (px: number) => `${(px / h) * 100}%`
  return {
    aspect: `${w} / ${h}`,
    titleBar: { height: ph(48) },
    sidebar: { left: pw(12), top: ph(48), width: pw(240), bottom: ph(12) },
    main: { left: pw(12 + 240 + 24), right: pw(24), top: ph(48 + 20), bottom: ph(32) },
  }
})

const previewEl = ref<HTMLElement | null>(null)

function pointerPercent(e: PointerEvent) {
  const rect = previewEl.value?.getBoundingClientRect()
  if (!rect || rect.width === 0 || rect.height === 0) return null
  return {
    x: ((e.clientX - rect.left) / rect.width) * 100,
    y: ((e.clientY - rect.top) / rect.height) * 100,
  }
}

// Offset between the pointer and the sticker's centre at grab time, so the
// sticker doesn't jump to centre itself under the cursor.
let drag: { id: string; dx: number; dy: number; moved: boolean } | null = null

function onStickerDown(e: PointerEvent, s: Sticker) {
  selectedId.value = s.id
  previewEl.value?.focus({ preventScroll: true })
  const p = pointerPercent(e)
  if (!p) return
  drag = { id: s.id, dx: p.x - s.x, dy: p.y - s.y, moved: false }
  ;(e.currentTarget as Element).setPointerCapture(e.pointerId)
}

function onStickerMove(e: PointerEvent) {
  if (!drag) return
  const p = pointerPercent(e)
  if (!p) return
  store.patch(drag.id, { x: p.x - drag.dx, y: p.y - drag.dy })
  drag.moved = true
}

function onStickerUp() {
  if (drag?.moved) store.persist()
  drag = null
}

function onKeydown(e: KeyboardEvent) {
  const s = selected.value
  if (!s) return
  if (e.key === 'Delete' || e.key === 'Backspace') {
    e.preventDefault()
    removeSelected()
    return
  }
  const step = e.shiftKey ? 2 : 0.5
  const moves: Record<string, [number, number]> = {
    ArrowLeft: [-step, 0],
    ArrowRight: [step, 0],
    ArrowUp: [0, -step],
    ArrowDown: [0, step],
  }
  const move = moves[e.key]
  if (!move) return
  e.preventDefault()
  store.patch(s.id, { x: s.x + move[0], y: s.y + move[1] })
  store.persist()
}

// Feedback for a click in the palette: a short notice over the preview,
// and the new sticker pops in, so it's clear something was added and where.
const notice = ref<{ text: string; kind: StickerArt | null } | null>(null)
let noticeTimer: ReturnType<typeof setTimeout> | undefined
function showNotice(text: string, kind: StickerArt | null) {
  notice.value = { text, kind }
  clearTimeout(noticeTimer)
  noticeTimer = setTimeout(() => (notice.value = null), 2200)
}

const poppedId = ref<string | null>(null)

function addSticker(kind: StickerArt) {
  const id = store.add(kind)
  if (!id) {
    showNotice(`That's the maximum of ${MAX_STICKERS} stickers.`, null)
    return
  }
  selectedId.value = id
  poppedId.value = id
  showNotice(`${stickerLabel(kind)} added — drag it into place`, kind)
}

function onSlider(field: 'size' | 'rotation', e: Event) {
  if (!selected.value) return
  store.patch(selected.value.id, { [field]: Number((e.target as HTMLInputElement).value) })
}

function flipSelected() {
  if (!selected.value) return
  store.patch(selected.value.id, { flip: !selected.value.flip })
  store.persist()
}

function duplicateSelected() {
  if (!selected.value) return
  const id = store.duplicate(selected.value.id)
  if (!id) return
  selectedId.value = id
  poppedId.value = id
  showNotice(`${selectedLabel.value} duplicated`, selected.value?.kind ?? null)
}

function removeSelected() {
  if (!selected.value) return
  store.remove(selected.value.id)
  selectedId.value = null
}

// "Remove all" asks for a second click instead of a dialog.
const confirmingClear = ref(false)
let clearTimer: ReturnType<typeof setTimeout> | undefined
function onClear() {
  if (!confirmingClear.value) {
    confirmingClear.value = true
    clearTimeout(clearTimer)
    clearTimer = setTimeout(() => (confirmingClear.value = false), 3000)
    return
  }
  clearTimeout(clearTimer)
  confirmingClear.value = false
  selectedId.value = null
  store.clear()
}
</script>

<template>
  <section>
    <div class="flex flex-wrap items-end justify-between gap-4">
      <div>
        <h1 class="text-2xl font-semibold text-neutral-900 dark:text-neutral-100">Decorations</h1>
        <p class="mt-1 text-sm text-neutral-500">
          Stick cute stickers anywhere on Rezure. Place them in the preview — they appear on the
          real window everywhere except this page.
        </p>
      </div>

      <div class="flex items-center gap-3">
        <span class="text-xs text-neutral-500">
          {{ store.stickers.length }}/{{ MAX_STICKERS }}
        </span>
        <label class="flex items-center gap-2 text-sm font-medium">
          Show stickers
          <button
            type="button"
            role="switch"
            :aria-checked="store.visible"
            class="relative h-6 w-11 shrink-0 rounded-full transition"
            :class="store.visible ? 'bg-accent-600' : 'bg-neutral-900/15 dark:bg-white/15'"
            @click="store.setVisible(!store.visible)"
          >
            <span
              class="absolute top-0.5 h-5 w-5 rounded-full bg-white shadow transition"
              :class="store.visible ? 'left-5' : 'left-0.5'"
            />
          </button>
        </label>
        <button
          type="button"
          class="glass-btn rounded-full px-3 py-1.5 text-xs font-semibold transition disabled:opacity-50"
          :class="confirmingClear ? 'text-red-600 dark:text-red-400' : ''"
          :disabled="store.stickers.length === 0"
          @click="onClear"
        >
          {{ confirmingClear ? 'Click again to remove all' : 'Remove all' }}
        </button>
      </div>
    </div>

    <p v-if="store.error" class="mt-3 text-sm text-red-600 dark:text-red-400">
      Couldn't save your stickers: {{ store.error }}
    </p>

    <div class="mt-6 grid gap-4 lg:grid-cols-[minmax(0,1fr)_16rem]">
      <div class="glass rounded-2xl p-3">
        <!-- A miniature of the window, in the current theme. Stickers are
             positioned in % of this box, the same way StickerOverlay places
             them in % of the window. -->
        <div
          ref="previewEl"
          tabindex="0"
          aria-label="Sticker preview. Arrow keys move the selected sticker, Delete removes it."
          class="bg-app relative w-full touch-none overflow-hidden rounded-xl outline-none select-none focus-visible:ring-2 focus-visible:ring-accent-400/60"
          :class="store.visible ? '' : 'opacity-60'"
          :style="{ aspectRatio: layout.aspect }"
          @pointerdown.self="selectedId = null"
          @keydown="onKeydown"
        >
          <div
            aria-hidden="true"
            class="pointer-events-none absolute inset-x-0 top-0 flex items-center gap-[0.6%] px-[1.3%]"
            :style="layout.titleBar"
          >
            <span class="aspect-square w-[1%] rounded-full bg-[#ff5f57]"></span>
            <span class="aspect-square w-[1%] rounded-full bg-[#febc2e]"></span>
            <span class="aspect-square w-[1%] rounded-full bg-[#28c840]"></span>
            <span
              class="ml-[1%] h-[22%] w-[6%] rounded-full bg-neutral-900/40 dark:bg-white/50"
            ></span>
            <span class="h-[18%] w-[5%] rounded-full bg-accent-500/70"></span>
          </div>

          <div
            aria-hidden="true"
            class="glass pointer-events-none absolute flex flex-col gap-[3%] rounded-[8%/4%] p-[1.2%]"
            :style="layout.sidebar"
          >
            <div
              v-for="i in 10"
              :key="i"
              class="flex h-[4.5%] items-center gap-[6%] rounded-md px-[5%]"
              :class="i === 1 ? 'glass-selected' : ''"
            >
              <span
                class="aspect-square h-[70%] rounded"
                :class="i === 1 ? 'bg-accent-500' : 'glass-inset'"
              ></span>
              <span class="h-[30%] flex-1 rounded-full bg-neutral-900/20 dark:bg-white/25"></span>
            </div>
          </div>

          <div
            aria-hidden="true"
            class="pointer-events-none absolute flex flex-col gap-[2.5%]"
            :style="layout.main"
          >
            <span class="h-[3.5%] w-[22%] rounded-full bg-neutral-900/35 dark:bg-white/45"></span>
            <span class="h-[2%] w-[40%] rounded-full bg-neutral-900/15 dark:bg-white/20"></span>
            <div
              v-for="i in 5"
              :key="i"
              class="glass flex h-[11%] items-center gap-[2%] rounded-lg px-[2%]"
            >
              <span class="glass-inset aspect-square h-[45%] rounded-md"></span>
              <span class="h-[14%] w-[24%] rounded-full bg-neutral-900/25 dark:bg-white/35"></span>
              <span class="ml-auto h-[30%] w-[10%] rounded-full bg-accent-500/80"></span>
            </div>
          </div>

          <img
            v-for="s in drawn"
            :key="s.id"
            :src="stickerUrl(s.kind)"
            alt=""
            draggable="false"
            class="sticker cursor-grab active:cursor-grabbing"
            :class="[
              s.id === selectedId
                ? 'outline-2 outline-offset-2 outline-accent-500 outline-dashed'
                : '',
              s.id === poppedId ? 'sticker-pop' : '',
            ]"
            :style="stickerStyle(s)"
            @pointerdown.stop="onStickerDown($event, s)"
            @pointermove="onStickerMove"
            @pointerup="onStickerUp"
            @pointercancel="onStickerUp"
            @animationend="poppedId = null"
          />

          <p
            v-if="store.stickers.length === 0"
            class="pointer-events-none absolute inset-0 flex items-center justify-center text-sm font-medium text-neutral-500"
          >
            Pick a sticker to start decorating
          </p>

          <Transition name="notice">
            <div
              v-if="notice"
              class="glass-strong pointer-events-none absolute bottom-[4%] left-1/2 flex -translate-x-1/2 items-center gap-2 rounded-full py-1.5 pr-4 pl-1.5 text-sm font-semibold whitespace-nowrap text-neutral-900 dark:text-neutral-100"
            >
              <span class="flex h-7 w-7 items-center justify-center rounded-full bg-accent-500/15">
                <img v-if="notice.kind" :src="stickerUrl(notice.kind)" alt="" class="h-5 w-5" />
                <svg
                  v-else
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="2.5"
                  class="h-4 w-4 text-red-500"
                >
                  <path stroke-linecap="round" d="M12 7v6m0 4h.01" />
                </svg>
              </span>
              {{ notice.text }}
            </div>
          </Transition>
        </div>

        <!-- Same message for screen readers, which can't see the toast. -->
        <p class="sr-only" aria-live="polite">{{ notice?.text ?? '' }}</p>

        <p class="mt-2 px-1 text-xs text-neutral-500">
          Drag to move · Arrow keys nudge (Shift for bigger steps) · Delete removes
        </p>
      </div>

      <div class="flex flex-col gap-4">
        <div class="glass rounded-2xl p-3">
          <!-- Wraps: with the Downloaded tab the row no longer fits beside the
               title in the 16rem palette card, and clipped off its edge. -->
          <div class="flex flex-wrap items-center justify-between gap-x-2 gap-y-1.5 px-1">
            <p class="text-sm font-semibold text-neutral-900 dark:text-neutral-100">Stickers</p>
            <div class="glass-inset flex gap-0.5 rounded-full p-0.5">
              <button
                v-for="tab in categoryTabs"
                :key="tab"
                type="button"
                class="rounded-full px-2.5 py-0.5 text-[11px] font-semibold transition"
                :class="
                  category === tab
                    ? 'glass-raised text-neutral-900 dark:text-neutral-100'
                    : 'glass-ghost'
                "
                @click="category = tab"
              >
                {{ tab }}
              </button>
            </div>
          </div>
          <div class="mt-1 flex items-center justify-between gap-2 px-1">
            <p class="text-xs text-neutral-500">
              {{ store.isFull ? `That's the maximum of ${MAX_STICKERS}.` : 'Click one to add it.' }}
            </p>
            <button
              type="button"
              class="glass-btn flex shrink-0 items-center gap-1 rounded-full px-2.5 py-1 text-[11px] font-semibold transition"
              @click="showBrowse = true"
            >
              <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2.5"
                class="h-3 w-3"
                aria-hidden="true"
              >
                <circle cx="11" cy="11" r="6.5" />
                <path stroke-linecap="round" d="M16 16l4.5 4.5" />
              </svg>
              Browse
            </button>
          </div>
          <p
            v-if="category === 'Downloaded' && downloaded.length === 0"
            class="mt-3 px-1 pb-1 text-xs text-neutral-500"
          >
            Nothing downloaded yet.
            <button
              type="button"
              class="font-semibold text-accent-600 underline-offset-2 hover:underline dark:text-accent-400"
              @click="showBrowse = true"
            >
              Browse the catalog
            </button>
            to find more stickers.
          </p>
          <!-- Fixed-size tiles, as many per row as fit, scrolling inside the
               card: at full width (below lg the palette sits under the
               preview) stretched columns made each tile huge. -->
          <div
            class="mt-2 grid max-h-64 grid-cols-[repeat(auto-fill,minmax(3.25rem,1fr))] gap-1.5 overflow-y-auto pr-1 [scrollbar-width:thin]"
          >
            <button
              v-for="info in visibleStickers"
              :key="info.kind"
              type="button"
              :title="info.label"
              :aria-label="`Add ${info.label}`"
              class="glass-inset flex aspect-square max-h-16 items-center justify-center rounded-xl p-1.5 transition hover:scale-105 disabled:opacity-40 disabled:hover:scale-100"
              :disabled="store.isFull"
              @click="addSticker(info.kind)"
            >
              <img :src="info.url" alt="" draggable="false" class="h-full w-full" />
            </button>
          </div>
        </div>

        <div v-if="selected" class="glass rounded-2xl p-3">
          <div class="flex items-center gap-2 px-1">
            <img :src="stickerUrl(selected.kind)" alt="" class="h-7 w-7" />
            <p class="text-sm font-semibold text-neutral-900 dark:text-neutral-100">
              {{ selectedLabel }}
            </p>
          </div>

          <label class="mt-3 block px-1 text-xs font-medium text-neutral-500">
            <span class="flex justify-between">
              Size <span>{{ selected.size.toFixed(1) }}%</span>
            </span>
            <input
              type="range"
              :min="MIN_SIZE"
              :max="MAX_SIZE"
              step="0.5"
              :value="selected.size"
              class="mt-1 w-full"
              @input="onSlider('size', $event)"
              @change="store.persist()"
            />
          </label>

          <label class="mt-2 block px-1 text-xs font-medium text-neutral-500">
            <span class="flex justify-between">
              Rotation <span>{{ Math.round(selected.rotation) }}°</span>
            </span>
            <input
              type="range"
              min="-180"
              max="180"
              step="1"
              :value="selected.rotation"
              class="mt-1 w-full"
              @input="onSlider('rotation', $event)"
              @change="store.persist()"
            />
          </label>

          <div class="mt-3 grid grid-cols-2 gap-1.5">
            <button
              type="button"
              class="glass-btn rounded-full px-3 py-1.5 text-xs font-semibold transition"
              @click="flipSelected"
            >
              Flip
            </button>
            <button
              type="button"
              class="glass-btn rounded-full px-3 py-1.5 text-xs font-semibold transition disabled:opacity-50"
              :disabled="store.isFull"
              @click="duplicateSelected"
            >
              Duplicate
            </button>
            <button
              type="button"
              class="glass-btn rounded-full px-3 py-1.5 text-xs font-semibold transition"
              @click="store.bringToFront(selected.id)"
            >
              Bring to front
            </button>
            <button
              type="button"
              class="glass-btn rounded-full px-3 py-1.5 text-xs font-semibold text-red-600 transition dark:text-red-400"
              @click="removeSelected"
            >
              Remove
            </button>
          </div>
        </div>
      </div>
    </div>

    <StickerBrowseModal v-if="showBrowse" @close="showBrowse = false" />
  </section>
</template>

<style scoped>
/* A just-added sticker pops in. `scale` rather than `transform`, which the
   inline style already uses for position and rotation. */
.sticker-pop {
  animation: sticker-pop 0.45s cubic-bezier(0.34, 1.56, 0.64, 1);
}

@keyframes sticker-pop {
  from {
    scale: 0.3;
    opacity: 0;
  }
  60% {
    scale: 1.15;
    opacity: 1;
  }
  to {
    scale: 1;
  }
}

.notice-enter-active,
.notice-leave-active {
  /* `transform`, not `translate`: Tailwind's -translate-x-1/2 centres the
     notice through the `translate` property, which this must not fight. */
  transition:
    opacity 0.2s ease,
    transform 0.2s ease;
}

.notice-enter-from,
.notice-leave-to {
  opacity: 0;
  transform: translateY(8px);
}
</style>
