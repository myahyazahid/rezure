import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { Decorations, Settings, Sticker, StickerKind } from '@/types/settings'

export type StickerCategory = 'Girls' | 'Mens'

export const STICKER_CATEGORIES: readonly StickerCategory[] = ['Girls', 'Mens']

export interface StickerInfo {
  kind: StickerKind
  label: string
  category: StickerCategory
  url: string
}

// Bundled by Vite; small enough that each ends up inlined as a data URI.
const ART = import.meta.glob<string>('../assets/stickers/*.svg', {
  eager: true,
  query: '?url',
  import: 'default',
})

function artUrl(kind: StickerKind): string {
  return ART[`../assets/stickers/${kind}.svg`] ?? ''
}

const CATALOG: readonly [StickerKind, string, StickerCategory][] = [
  ['bow', 'Pink bow', 'Girls'],
  ['heart', 'Heart', 'Girls'],
  ['sparkle', 'Sparkle', 'Girls'],
  ['star', 'Star', 'Girls'],
  ['sakura', 'Sakura', 'Girls'],
  ['cloud', 'Cloud', 'Girls'],
  ['strawberry', 'Strawberry', 'Girls'],
  ['cat', 'Kitty', 'Girls'],
  ['butterfly', 'Butterfly', 'Girls'],
  ['rainbow', 'Rainbow', 'Girls'],
  ['crown', 'Crown', 'Girls'],
  ['cherry', 'Cherries', 'Girls'],
  ['gamepad', 'Gamepad', 'Mens'],
  ['rocket', 'Rocket', 'Mens'],
  ['bolt', 'Lightning', 'Mens'],
  ['flame', 'Flame', 'Mens'],
  ['coffee', 'Coffee', 'Mens'],
  ['terminal', 'Terminal', 'Mens'],
  ['football', 'Football', 'Mens'],
  ['headphones', 'Headphones', 'Mens'],
  ['shield', 'Shield', 'Mens'],
  ['robot', 'Robot', 'Mens'],
  ['planet', 'Planet', 'Mens'],
  ['sunglasses', 'Sunglasses', 'Mens'],
]

/** The sticker palette on the Decorations page, in display order. */
export const STICKERS: readonly StickerInfo[] = CATALOG.map(([kind, label, category]) => ({
  kind,
  label,
  category,
  url: artUrl(kind),
}))

export function stickerLabel(kind: StickerKind): string {
  return STICKERS.find((s) => s.kind === kind)?.label ?? 'Sticker'
}

/** Mirrors the limits in `config::stickers` — Rust clamps anyway; these keep
 *  the controls from offering values that would be clamped away. */
export const MAX_STICKERS = 40
export const MIN_SIZE = 2
export const MAX_SIZE = 25
export const DEFAULT_SIZE = 7

export function stickerUrl(kind: StickerKind): string {
  return artUrl(kind)
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
  const stickers = ref<Sticker[]>([])
  const error = ref<string | null>(null)

  const isFull = computed(() => stickers.value.length >= MAX_STICKERS)

  function apply(value: Decorations) {
    visible.value = value.visible
    stickers.value = value.stickers
  }

  async function load() {
    try {
      apply((await invoke<Settings>('get_settings')).decorations)
    } catch (e) {
      error.value = errorMessage(e)
    }
  }

  /** Saves the arrangement as it is on screen. Not applied back from the
   *  reply, for the same reason as the appearance store: a slow reply would
   *  undo a change made after it was sent. */
  async function persist() {
    error.value = null
    try {
      await invoke<Settings>('update_settings', {
        patch: { decorations: { visible: visible.value, stickers: stickers.value } },
      })
    } catch (e) {
      error.value = errorMessage(e)
    }
  }

  function find(id: string) {
    return stickers.value.find((s) => s.id === id)
  }

  /** Adds a sticker near the middle, nudged so a few added in a row don't
   *  land exactly on top of each other. Returns its id, or null when full. */
  function add(kind: StickerKind): string | null {
    if (isFull.value) return null
    const jitter = () => (Math.random() - 0.5) * 16
    const sticker: Sticker = {
      id: newId(),
      kind,
      x: 50 + jitter(),
      y: 50 + jitter(),
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

  function setVisible(value: boolean) {
    visible.value = value
    persist()
  }

  return {
    visible,
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
    setVisible,
  }
})
