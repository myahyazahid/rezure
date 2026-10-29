<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import AppWindowMiniature from '@/components/decorations/AppWindowMiniature.vue'
import StickerBrowseModal from '@/components/decorations/StickerBrowseModal.vue'
import {
  DEFAULT_SIZE,
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
import { useAppearanceStore } from '@/stores/appearance'
import type { Sticker, StickerArt } from '@/types/settings'

const store = useDecorationsStore()
const library = useStickerLibraryStore()
const appearance = useAppearanceStore()

/** Whether Live stickers are moving — what the Animate switch shows. */
const animating = computed(() => store.animate && !appearance.reduceMotion)

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

type PaletteTab = StickerCategory | 'Downloaded'

const showBrowse = ref(false)

const category = ref<PaletteTab>('Live')
const categoryTabs: readonly PaletteTab[] = [...STICKER_CATEGORIES, 'Downloaded']
const downloaded = computed<PaletteItem[]>(() =>
  library.saved.map((s) => ({ kind: savedKind(s.id), label: s.name, url: s.dataUrl })),
)
const visibleStickers = computed<PaletteItem[]>(() => {
  if (category.value === 'Downloaded') return downloaded.value
  return STICKERS.filter((s) => s.category === category.value).map((s) => ({
    kind: s.kind,
    label: s.label,
    url: stickerUrl(s.kind),
  }))
})

// The preview has the window's own proportions and layout, so a sticker put
// somewhere in it lands on the same spot of the real window. Tracked live, so
// resizing the window reshapes the preview with it.
const windowSize = ref({ w: window.innerWidth, h: window.innerHeight })
function onResize() {
  windowSize.value = { w: window.innerWidth, h: window.innerHeight }
}

const previewEl = ref<HTMLElement | null>(null)

// The miniature is drawn at the window's full size and scaled down to the
// preview's width, so it follows the preview box, not just the window.
const previewWidth = ref(0)
const previewScale = computed(() => previewWidth.value / windowSize.value.w)
const previewObserver = new ResizeObserver(([entry]) => {
  if (entry) previewWidth.value = entry.contentRect.width
})

onMounted(() => {
  window.addEventListener('resize', onResize)
  if (previewEl.value) previewObserver.observe(previewEl.value)
})
onUnmounted(() => {
  window.removeEventListener('resize', onResize)
  previewObserver.disconnect()
})

function pointerPercent(e: PointerEvent) {
  const rect = previewEl.value?.getBoundingClientRect()
  if (!rect || rect.width === 0 || rect.height === 0) return null
  return {
    x: ((e.clientX - rect.left) / rect.width) * 100,
    y: ((e.clientY - rect.top) / rect.height) * 100,
  }
}

/** A sticker's centre in client pixels, for the resize and rotate handles. */
function centrePx(s: Sticker) {
  const rect = previewEl.value?.getBoundingClientRect()
  if (!rect) return null
  return { x: rect.left + (s.x / 100) * rect.width, y: rect.top + (s.y / 100) * rect.height }
}

/** What a pointer drag in the preview is doing to the selected sticker. A
 *  move keeps the offset between the pointer and the sticker's centre at
 *  grab time, so the sticker doesn't jump to centre itself under the
 *  cursor; a resize scales by how far the pointer is from the centre
 *  compared with where it started, so it works the same at any rotation. */
type Drag =
  | { mode: 'move'; id: string; dx: number; dy: number; moved: boolean }
  | { mode: 'resize'; id: string; startSize: number; startDist: number; moved: boolean }
  | { mode: 'rotate'; id: string; moved: boolean }

let drag: Drag | null = null

/** Rotation steps when Shift is held on the rotate handle. */
const ROTATION_SNAP = 15

/** The resize handles, centred on the frame's corners (the frame sits
 *  1.5 outside the sticker, the handle is 3 wide). */
const RESIZE_CORNERS = [
  { position: '-top-3 -left-3', cursor: 'cursor-nwse-resize' },
  { position: '-top-3 -right-3', cursor: 'cursor-nesw-resize' },
  { position: '-bottom-3 -left-3', cursor: 'cursor-nesw-resize' },
  { position: '-bottom-3 -right-3', cursor: 'cursor-nwse-resize' },
] as const

/** Where the selection frame goes: the sticker's own box and rotation, but
 *  never flipped, so the rotate handle stays on top. */
const selectionStyle = computed(() => {
  const s = selected.value
  if (!s) return {}
  return {
    left: `${s.x}%`,
    top: `${s.y}%`,
    width: `${s.size}%`,
    transform: `translate(-50%, -50%) rotate(${s.rotation}deg)`,
  }
})

function onStickerDown(e: PointerEvent, s: Sticker) {
  selectedId.value = s.id
  previewEl.value?.focus({ preventScroll: true })
  const p = pointerPercent(e)
  if (!p) return
  drag = { mode: 'move', id: s.id, dx: p.x - s.x, dy: p.y - s.y, moved: false }
  ;(e.currentTarget as Element).setPointerCapture(e.pointerId)
}

function onHandleDown(e: PointerEvent, mode: 'resize' | 'rotate') {
  const s = selected.value
  const c = s && centrePx(s)
  if (!s || !c) return
  previewEl.value?.focus({ preventScroll: true })
  drag =
    mode === 'resize'
      ? {
          mode,
          id: s.id,
          startSize: s.size,
          startDist: Math.max(1, Math.hypot(e.clientX - c.x, e.clientY - c.y)),
          moved: false,
        }
      : { mode, id: s.id, moved: false }
  ;(e.currentTarget as Element).setPointerCapture(e.pointerId)
}

function onStickerMove(e: PointerEvent) {
  if (!drag) return
  if (drag.mode === 'move') {
    const p = pointerPercent(e)
    if (!p) return
    store.patch(drag.id, { x: p.x - drag.dx, y: p.y - drag.dy })
    trash.value = isOverTrash(e) ? 'armed' : p.y >= TRASH_SHOWN_AT ? 'shown' : 'hidden'
  } else {
    const s = store.stickers.find((st) => st.id === drag?.id)
    const c = s && centrePx(s)
    if (!c) return
    const dx = e.clientX - c.x
    const dy = e.clientY - c.y
    if (drag.mode === 'resize') {
      store.patch(drag.id, { size: (drag.startSize * Math.hypot(dx, dy)) / drag.startDist })
    } else {
      // The handle sits straight above the centre, so "up" is 0°.
      let angle = (Math.atan2(dy, dx) * 180) / Math.PI + 90
      if (e.shiftKey) angle = Math.round(angle / ROTATION_SNAP) * ROTATION_SNAP
      angle = Math.round(angle)
      if (angle > 180) angle -= 360
      store.patch(drag.id, { rotation: angle })
    }
  }
  drag.moved = true
}

function onStickerUp() {
  if (drag?.mode === 'move' && trash.value === 'armed') {
    store.remove(drag.id)
    if (selectedId.value === drag.id) selectedId.value = null
  } else if (drag?.moved) {
    store.persist()
  }
  drag = null
  trash.value = 'hidden'
}

// The bin at the bottom of the preview. It shows once the pointer moving a
// sticker gets near the bottom edge (in percent of the preview's height),
// and letting go over it — or anywhere below the preview — removes the
// sticker. Only the bin itself removes, so a sticker can still be placed
// along the rest of the bottom edge.
const TRASH_SHOWN_AT = 75
/** Extra room around the bin that still counts as over it, in pixels. */
const TRASH_SLOP = 12
const trash = ref<'hidden' | 'shown' | 'armed'>('hidden')
const trashEl = ref<HTMLElement | null>(null)

function isOverTrash(e: PointerEvent) {
  const preview = previewEl.value?.getBoundingClientRect()
  if (preview && e.clientY > preview.bottom) return true
  const bin = trashEl.value?.getBoundingClientRect()
  return (
    !!bin &&
    e.clientX >= bin.left - TRASH_SLOP &&
    e.clientX <= bin.right + TRASH_SLOP &&
    e.clientY >= bin.top - TRASH_SLOP &&
    e.clientY <= bin.bottom + TRASH_SLOP
  )
}

/** A sticker being dragged out of the palette. It only becomes a drag
 *  (and shows a copy under the pointer) after moving a few pixels, so a
 *  plain click still adds the sticker the old way. */
const paletteDrag = ref<{
  kind: StickerArt
  url: string
  startX: number
  startY: number
  x: number
  y: number
  active: boolean
  overPreview: boolean
} | null>(null)
const PALETTE_DRAG_THRESHOLD = 5
// A drag that ends on the tile it started from still fires a click there;
// this swallows that one click so the drop doesn't add a second sticker.
let swallowClick = false

/** The dragged copy is drawn at the size it will land at in the preview. */
const ghostWidth = computed(() => (previewWidth.value * DEFAULT_SIZE) / 100)

function isOverPreview(e: PointerEvent) {
  const rect = previewEl.value?.getBoundingClientRect()
  return (
    !!rect &&
    e.clientX >= rect.left &&
    e.clientX <= rect.right &&
    e.clientY >= rect.top &&
    e.clientY <= rect.bottom
  )
}

function onTileDown(e: PointerEvent, info: PaletteItem) {
  swallowClick = false
  if (e.button !== 0 || store.isFull) return
  paletteDrag.value = {
    kind: info.kind,
    url: info.url,
    startX: e.clientX,
    startY: e.clientY,
    x: e.clientX,
    y: e.clientY,
    active: false,
    overPreview: false,
  }
  ;(e.currentTarget as Element).setPointerCapture(e.pointerId)
}

function onTileMove(e: PointerEvent) {
  const d = paletteDrag.value
  if (!d) return
  d.x = e.clientX
  d.y = e.clientY
  if (
    !d.active &&
    Math.hypot(e.clientX - d.startX, e.clientY - d.startY) >= PALETTE_DRAG_THRESHOLD
  ) {
    d.active = true
  }
  d.overPreview = d.active && isOverPreview(e)
}

function onTileUp(e: PointerEvent) {
  const d = paletteDrag.value
  paletteDrag.value = null
  if (!d?.active) return
  swallowClick = true
  if (!isOverPreview(e)) return
  const p = pointerPercent(e)
  if (p) addSticker(d.kind, p)
}

function onTileClick(kind: StickerArt) {
  if (swallowClick) {
    swallowClick = false
    return
  }
  addSticker(kind)
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

/** Adds a sticker, at `at` when it was dropped onto the preview. */
function addSticker(kind: StickerArt, at?: { x: number; y: number }) {
  const id = store.add(kind, at)
  if (!id) {
    showNotice(`That's the maximum of ${MAX_STICKERS} stickers.`, null)
    return
  }
  selectedId.value = id
  poppedId.value = id
  if (at) previewEl.value?.focus({ preventScroll: true })
  showNotice(
    at ? `${stickerLabel(kind)} added` : `${stickerLabel(kind)} added — drag it into place`,
    kind,
  )
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
        <!-- Live stickers only; Reduce motion (Appearance) keeps them still
             whatever this says, so the switch shows off and can't be used. -->
        <label
          class="flex items-center gap-2 text-sm font-medium"
          :class="appearance.reduceMotion ? 'opacity-50' : ''"
          :title="appearance.reduceMotion ? 'Reduce motion is on in Appearance' : undefined"
        >
          Animate
          <button
            type="button"
            role="switch"
            :aria-checked="animating"
            :disabled="appearance.reduceMotion"
            class="relative h-6 w-11 shrink-0 rounded-full transition"
            :class="animating ? 'bg-accent-600' : 'bg-neutral-900/15 dark:bg-white/15'"
            @click="store.setAnimate(!store.animate)"
          >
            <span
              class="absolute top-0.5 h-5 w-5 rounded-full bg-white shadow transition"
              :class="animating ? 'left-5' : 'left-0.5'"
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
          :class="[
            store.visible ? '' : 'opacity-60',
            paletteDrag?.overPreview ? 'ring-2 ring-accent-500' : '',
          ]"
          :style="{ aspectRatio: `${windowSize.w} / ${windowSize.h}` }"
          @pointerdown.self="selectedId = null"
          @keydown="onKeydown"
        >
          <AppWindowMiniature :width="windowSize.w" :height="windowSize.h" :scale="previewScale" />

          <img
            v-for="s in drawn"
            :key="s.id"
            :src="stickerUrl(s.kind)"
            alt=""
            draggable="false"
            class="sticker cursor-grab active:cursor-grabbing"
            :class="[
              s.id === poppedId ? 'sticker-pop' : '',
              s.id === selectedId && trash === 'armed' ? 'opacity-40' : '',
            ]"
            :style="stickerStyle(s)"
            @pointerdown.stop="onStickerDown($event, s)"
            @pointermove="onStickerMove"
            @pointerup="onStickerUp"
            @pointercancel="onStickerUp"
            @animationend="poppedId = null"
          />

          <!-- The selected sticker's frame, over every sticker so its handles
               are never covered. Same box as the sticker without the flip;
               the hidden copy of the art gives it the sticker's height. The
               frame itself lets clicks through to the sticker, only the
               handles take them. -->
          <div
            v-if="selected && stickerUrl(selected.kind) && trash !== 'armed'"
            class="pointer-events-none absolute"
            :style="selectionStyle"
          >
            <img :src="stickerUrl(selected.kind)" alt="" class="invisible block w-full" />
            <div class="absolute -inset-1.5 rounded-lg border-2 border-accent-500"></div>

            <div
              class="absolute bottom-full left-1/2 mb-1.5 flex -translate-x-1/2 flex-col items-center"
            >
              <span
                title="Rotate (Shift snaps to 15°)"
                class="pointer-events-auto flex h-5 w-5 cursor-grab items-center justify-center rounded-full border-2 border-accent-500 bg-white text-accent-600 shadow active:cursor-grabbing"
                @pointerdown.stop="onHandleDown($event, 'rotate')"
                @pointermove="onStickerMove"
                @pointerup="onStickerUp"
                @pointercancel="onStickerUp"
              >
                <svg
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="3"
                  class="h-2.5 w-2.5"
                  aria-hidden="true"
                >
                  <path
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    d="M19.5 12a7.5 7.5 0 1 1-2.2-5.3L20 9M20 9V4M20 9h-5"
                  />
                </svg>
              </span>
              <span class="h-2.5 w-0.5 bg-accent-500"></span>
            </div>

            <span
              v-for="corner in RESIZE_CORNERS"
              :key="corner.position"
              title="Resize"
              class="pointer-events-auto absolute h-3 w-3 rounded-full border-2 border-accent-500 bg-white shadow"
              :class="[corner.position, corner.cursor]"
              @pointerdown.stop="onHandleDown($event, 'resize')"
              @pointermove="onStickerMove"
              @pointerup="onStickerUp"
              @pointercancel="onStickerUp"
            ></span>
          </div>

          <p
            v-if="store.stickers.length === 0"
            class="pointer-events-none absolute inset-0 flex items-center justify-center text-sm font-medium text-neutral-500"
          >
            Pick a sticker, or drag one in, to start decorating
          </p>

          <Transition name="trash">
            <div
              v-if="trash !== 'hidden'"
              ref="trashEl"
              class="pointer-events-none absolute bottom-[4%] left-1/2 flex -translate-x-1/2 items-center gap-2 rounded-full py-1.5 pr-4 pl-1.5 text-sm font-semibold whitespace-nowrap transition"
              :class="
                trash === 'armed'
                  ? 'scale-110 bg-red-600 text-white shadow-lg'
                  : 'glass-strong text-neutral-700 dark:text-neutral-200'
              "
            >
              <span
                class="flex h-7 w-7 items-center justify-center rounded-full"
                :class="trash === 'armed' ? 'bg-white/20' : 'bg-red-500/15 text-red-600'"
              >
                <svg
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="2"
                  class="h-4 w-4"
                  aria-hidden="true"
                >
                  <path
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    d="M4 7h16M10 11v6M14 11v6M5 7l1 12a2 2 0 0 0 2 2h8a2 2 0 0 0 2-2l1-12M9 7V4h6v3"
                  />
                </svg>
              </span>
              {{ trash === 'armed' ? 'Release to remove' : 'Drag here to remove' }}
            </div>
          </Transition>

          <Transition name="notice">
            <div
              v-if="notice && trash === 'hidden'"
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
          Drag stickers in from the list · Corners resize · Top handle rotates (Shift snaps) · Drag
          to the bottom or press Delete to remove
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
              class="glass-inset flex aspect-square max-h-16 cursor-grab touch-none items-center justify-center rounded-xl p-1.5 transition hover:scale-105 disabled:cursor-default disabled:opacity-40 disabled:hover:scale-100"
              :class="paletteDrag?.active && paletteDrag.kind === info.kind ? 'opacity-40' : ''"
              :disabled="store.isFull"
              @pointerdown="onTileDown($event, info)"
              @pointermove="onTileMove"
              @pointerup="onTileUp"
              @pointercancel="paletteDrag = null"
              @click="onTileClick(info.kind)"
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

    <!-- The sticker being dragged out of the palette, under the pointer.
         On <body>: a fixed element inside the page would be offset by any
         transformed ancestor. -->
    <Teleport to="body">
      <img
        v-if="paletteDrag?.active"
        :src="paletteDrag.url"
        alt=""
        draggable="false"
        class="sticker pointer-events-none z-50 transition-opacity"
        :class="paletteDrag.overPreview ? 'opacity-100' : 'opacity-60'"
        :style="{
          position: 'fixed',
          left: `${paletteDrag.x}px`,
          top: `${paletteDrag.y}px`,
          width: `${ghostWidth}px`,
          transform: 'translate(-50%, -50%) rotate(-6deg)',
        }"
      />
    </Teleport>
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
.notice-leave-active,
.trash-enter-active,
.trash-leave-active {
  /* `transform`, not `translate`: Tailwind's -translate-x-1/2 centres the
     notice through the `translate` property, which this must not fight. */
  transition:
    opacity 0.2s ease,
    transform 0.2s ease;
}

.notice-enter-from,
.notice-leave-to,
.trash-enter-from,
.trash-leave-to {
  opacity: 0;
  transform: translateY(8px);
}
</style>
