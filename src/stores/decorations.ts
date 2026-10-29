import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type {
  Decorations,
  SavedStickerKind,
  Settings,
  Sticker,
  StickerArt,
  StickerKind,
} from '@/types/settings'
import { useStickerLibraryStore } from '@/stores/stickerLibrary'
import { useAppearanceStore } from '@/stores/appearance'

/** The palette's tabs for built-in art: Live (animated) and Classic (still). */
export type StickerCategory = 'Live' | 'Classic'

export const STICKER_CATEGORIES: readonly StickerCategory[] = ['Live', 'Classic']

export interface StickerInfo {
  kind: StickerKind
  label: string
  category: StickerCategory
}

// Bundled by Vite; small enough that each ends up inlined as a data URI.
const CLASSIC_ART = import.meta.glob<string>('../assets/stickers/*.svg', {
  eager: true,
  query: '?url',
  import: 'default',
})

// Live art carries its own CSS animation, which nothing outside an <img> can
// reach — not the "reduce-motion" class, not a paused animation. So each is
// kept twice: as drawn, and with every animation switched off.
const LIVE_SOURCE = import.meta.glob<string>('../assets/stickers/live/*.svg', {
  eager: true,
  query: '?raw',
  import: 'default',
})

const STILL_STYLE = '<style>*{animation:none!important}</style>'

function svgDataUrl(svg: string): string {
  return `data:image/svg+xml,${encodeURIComponent(svg)}`
}

const LIVE_ART = new Map(
  Object.entries(LIVE_SOURCE).map(([path, svg]) => [
    path.slice(path.lastIndexOf('/') + 1, -'.svg'.length),
    { moving: svgDataUrl(svg), still: svgDataUrl(svg.replace('</svg>', `${STILL_STYLE}</svg>`)) },
  ]),
)

/** Whether Live stickers are drawn moving right now. */
function liveMoves(): boolean {
  return useDecorationsStore().animate && !useAppearanceStore().reduceMotion
}

function artUrl(kind: StickerKind): string {
  const live = LIVE_ART.get(kind)
  if (live) return liveMoves() ? live.moving : live.still
  return CLASSIC_ART[`../assets/stickers/${kind}.svg`] ?? ''
}

const CATALOG: readonly [StickerKind, string, StickerCategory][] = [
  ['baby-chick', 'Baby chick', 'Live'],
  ['baby-panda', 'Baby panda', 'Live'],
  ['baby-frog', 'Baby frog', 'Live'],
  ['baby-kitten', 'Sleepy kitten', 'Live'],
  ['baby-hedgehog', 'Baby hedgehog', 'Live'],
  ['baby-hamster', 'Baby hamster', 'Live'],
  ['baby-penguin', 'Baby penguin', 'Live'],
  ['baby-bunny', 'Baby bunny', 'Live'],
  ['baby-lion', 'Lion cub', 'Live'],
  ['baby-trex', 'Baby T-rex', 'Live'],
  ['baby-duck', 'Duckling', 'Live'],
  ['baby-capybara', 'Baby capybara', 'Live'],
  ['baby-bear', 'Bear cub', 'Live'],
  ['baby-piglet', 'Piglet', 'Live'],
  ['baby-puppy', 'Puppy', 'Live'],
  ['baby-seal', 'Seal pup', 'Live'],
  ['baby-axolotl', 'Axolotl', 'Live'],
  ['baby-koala', 'Baby koala', 'Live'],
  ['baby-otter', 'Baby otter', 'Live'],
  ['baby-sloth', 'Baby sloth', 'Live'],
  ['baby-whale', 'Baby whale', 'Live'],
  ['baby-kitsune', 'Little kitsune', 'Live'],
  ['bow', 'Pink bow', 'Classic'],
  ['heart', 'Heart', 'Classic'],
  ['sparkle', 'Sparkle', 'Classic'],
  ['star', 'Star', 'Classic'],
  ['sakura', 'Sakura', 'Classic'],
  ['cloud', 'Cloud', 'Classic'],
  ['strawberry', 'Strawberry', 'Classic'],
  ['cat', 'Kitty', 'Classic'],
  ['butterfly', 'Butterfly', 'Classic'],
  ['rainbow', 'Rainbow', 'Classic'],
  ['crown', 'Crown', 'Classic'],
  ['cherry', 'Cherries', 'Classic'],
  ['gamepad', 'Gamepad', 'Classic'],
  ['rocket', 'Rocket', 'Classic'],
  ['bolt', 'Lightning', 'Classic'],
  ['flame', 'Flame', 'Classic'],
  ['coffee', 'Coffee', 'Classic'],
  ['terminal', 'Terminal', 'Classic'],
  ['football', 'Football', 'Classic'],
  ['headphones', 'Headphones', 'Classic'],
  ['shield', 'Shield', 'Classic'],
  ['robot', 'Robot', 'Classic'],
  ['planet', 'Planet', 'Classic'],
  ['sunglasses', 'Sunglasses', 'Classic'],
]

/** The sticker palette on the Decorations page, in display order. Images
 *  come from `stickerUrl`, which picks a Live sticker's moving or still art. */
export const STICKERS: readonly StickerInfo[] = CATALOG.map(([kind, label, category]) => ({
  kind,
  label,
  category,
}))

const SAVED_PREFIX = 'saved:'

/** The kind a downloaded sticker is placed as. */
export function savedKind(id: string): SavedStickerKind {
  return `${SAVED_PREFIX}${id}`
}

/** The library id of a downloaded sticker's kind, or `null` for built-in art. */
export function savedIdOf(kind: StickerArt): string | null {
  return kind.startsWith(SAVED_PREFIX) ? kind.slice(SAVED_PREFIX.length) : null
}

export function stickerLabel(kind: StickerArt): string {
  const id = savedIdOf(kind)
  if (id !== null) return useStickerLibraryStore().nameFor(id) ?? 'Sticker'
  return STICKERS.find((s) => s.kind === kind)?.label ?? 'Sticker'
}

/** Mirrors the limits in `config::stickers` — Rust clamps anyway; these keep
 *  the controls from offering values that would be clamped away. */
export const MAX_STICKERS = 40
export const MIN_SIZE = 2
export const MAX_SIZE = 25
export const DEFAULT_SIZE = 7

/** The image for a sticker, or `''` when there isn't one to draw — a
 *  download that was removed, or the library not loaded yet. Callers skip
 *  stickers with no image. */
export function stickerUrl(kind: StickerArt): string {
  const id = savedIdOf(kind)
  if (id !== null) return useStickerLibraryStore().urlFor(id)
  return artUrl(kind as StickerKind)
}

/** Where and how a sticker is drawn inside a box that stands for the window
 *  (the real window, or the preview). Everything is relative to that box, so
 *  one formula serves both. */
export function stickerStyle(s: Sticker): Record<string, string> {
  return {
    left: `${s.x}%`,
    top: `${s.y}%`,
    width: `${s.size}%`,
    transform: `translate(-50%, -50%) rotate(${s.rotation}deg) scaleX(${s.flip ? -1 : 1})`,
  }
}

function errorMessage(e: unknown): string {
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  return 'Something went wrong.'
}

function newId(): string {
  try {
    return crypto.randomUUID()
  } catch {
    return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`
  }
}

export const useDecorationsStore = defineStore('decorations', () => {
  const visible = ref(true)
  const animate = ref(true)
  const stickers = ref<Sticker[]>([])
  const error = ref<string | null>(null)

  const isFull = computed(() => stickers.value.length >= MAX_STICKERS)

  function apply(value: Decorations) {
    visible.value = value.visible
    animate.value = value.animate
    stickers.value = value.stickers
  }

  async function load() {
    try {
      apply((await invoke<Settings>('get_settings')).decorations)
    } catch (e) {
      error.value = errorMessage(e)
      return
    }
    // Downloaded stickers are drawn from the library, so it's read too — and
    // only then can a placed copy of a download that's gone be told apart
    // from one that simply hasn't loaded yet.
    if (await useStickerLibraryStore().loadSaved()) pruneMissing()
  }

  /** Saves the arrangement as it is on screen. Not applied back from the
   *  reply, for the same reason as the appearance store: a slow reply would
   *  undo a change made after it was sent. */
  async function persist() {
    error.value = null
    try {
      await invoke<Settings>('update_settings', {
        patch: {
          decorations: {
            visible: visible.value,
            animate: animate.value,
            stickers: stickers.value,
          },
        },
      })
    } catch (e) {
      error.value = errorMessage(e)
    }
  }

  function find(id: string) {
    return stickers.value.find((s) => s.id === id)
  }

  /** Adds a sticker at `at` (percent of the window, for one dropped onto
   *  the preview), or near the middle, nudged so a few added in a row don't
   *  land exactly on top of each other. Returns its id, or null when full. */
  function add(kind: StickerArt, at?: { x: number; y: number }): string | null {
    if (isFull.value) return null
    const jitter = () => (Math.random() - 0.5) * 16
    const sticker: Sticker = {
      id: newId(),
      kind,
      x: at ? Math.min(100, Math.max(0, at.x)) : 50 + jitter(),
      y: at ? Math.min(100, Math.max(0, at.y)) : 50 + jitter(),
      size: DEFAULT_SIZE,
      rotation: Math.round((Math.random() - 0.5) * 24),
      flip: false,
    }
    stickers.value.push(sticker)
    persist()
    return sticker.id
  }

  /** Changes a sticker on screen only — for dragging and sliders, which
   *  call `persist()` once they are let go. */
  function patch(id: string, changes: Partial<Omit<Sticker, 'id' | 'kind'>>) {
    const s = find(id)
    if (!s) return
    Object.assign(s, changes)
    s.x = Math.min(100, Math.max(0, s.x))
    s.y = Math.min(100, Math.max(0, s.y))
    s.size = Math.min(MAX_SIZE, Math.max(MIN_SIZE, s.size))
  }

  function remove(id: string) {
    stickers.value = stickers.value.filter((s) => s.id !== id)
    persist()
  }

  function duplicate(id: string): string | null {
    const s = find(id)
    if (!s || isFull.value) return null
    const copy = { ...s, id: newId(), x: Math.min(100, s.x + 4), y: Math.min(100, s.y + 4) }
    stickers.value.push(copy)
    persist()
    return copy.id
  }

  function bringToFront(id: string) {
    const s = find(id)
    if (!s) return
    stickers.value = [...stickers.value.filter((x) => x.id !== id), s]
    persist()
  }

  function clear() {
    stickers.value = []
    persist()
  }

  /** How many copies of a downloaded sticker are on the window. */
  function placedCount(id: string): number {
    return stickers.value.filter((s) => s.kind === savedKind(id)).length
  }

  /** Deletes a download and every copy of it on the window. The copies go
   *  only once the download is really gone, so a failed delete leaves things
   *  as they were. */
  async function removeSaved(id: string): Promise<boolean> {
    if (!(await useStickerLibraryStore().remove(id))) return false
    const kept = stickers.value.filter((s) => s.kind !== savedKind(id))
    if (kept.length !== stickers.value.length) {
      stickers.value = kept
      persist()
    }
    return true
  }

  /** Drops placed copies of downloads that no longer exist — a file deleted
   *  by hand, say. They'd draw nothing but still count toward the limit. */
  function pruneMissing() {
    const library = useStickerLibraryStore()
    const kept = stickers.value.filter((s) => {
      const id = savedIdOf(s.kind)
      return id === null || library.urlFor(id) !== ''
    })
    if (kept.length !== stickers.value.length) {
      stickers.value = kept
      persist()
    }
  }

  function setVisible(value: boolean) {
    visible.value = value
    persist()
  }

  function setAnimate(value: boolean) {
    animate.value = value
    persist()
  }

  return {
    visible,
    animate,
    stickers,
    error,
    isFull,
    load,
    persist,
    add,
    patch,
    remove,
    duplicate,
    bringToFront,
    clear,
    placedCount,
    removeSaved,
    setVisible,
    setAnimate,
  }
})
