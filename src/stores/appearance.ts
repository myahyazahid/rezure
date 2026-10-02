import { defineStore } from 'pinia'
import { computed, ref, watchEffect } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import type { AppearanceSettings, Settings, ThemeMode, ThemePreset } from '@/types/settings'

export type ThemeCategory = 'Default' | 'Girls' | 'Mens'

export interface ThemeInfo {
  id: ThemePreset
  name: string
  category: ThemeCategory
  description: string
}

/** The themes, grouped by category on the page. The colours are in `main.css`, under
 *  `[data-theme='<id>']`; this is only what the Appearance page lists. */
export const THEMES: readonly ThemeInfo[] = [
  {
    id: 'rezure',
    name: 'Rezure',
    category: 'Default',
    description: 'Coral red over a warm glass backdrop.',
  },
  {
    id: 'blossom',
    name: 'Blossom',
    category: 'Girls',
    description: 'Pink and lavender, scattered with petals.',
  },
  {
    id: 'softpink',
    name: 'Soft Pink',
    category: 'Girls',
    description: 'Clear, airy glass over soft pink, with bubbles.',
  },
  {
    id: 'lavender',
    name: 'Lavender Dream',
    category: 'Girls',
    description: 'Lilac and baby blue, with a crescent moon and stars.',
  },
  {
    id: 'peach',
    name: 'Peach',
    category: 'Girls',
    description: 'Warm peach and cream, with little hearts.',
  },
  {
    id: 'matcha',
    name: 'Matcha',
    category: 'Girls',
    description: 'Soft mint and sage, with tiny leaves.',
  },
  {
    id: 'midnight',
    name: 'Midnight',
    category: 'Mens',
    description: 'Navy and teal with a faint grid.',
  },
  {
    id: 'navy',
    name: 'Navy',
    category: 'Mens',
    description: 'Deep navy with a touch of gold and soft waves.',
  },
  {
    id: 'carbon',
    name: 'Carbon',
    category: 'Mens',
    description: 'Graphite and orange, with a carbon-fibre weave.',
  },
  {
    id: 'forest',
    name: 'Forest',
    category: 'Mens',
    description: 'Olive and deep green, with map contour lines.',
  },
  {
    id: 'terminal',
    name: 'Terminal',
    category: 'Mens',
    description: 'Neon green on black, with CRT scanlines.',
  },
]

export const THEME_CATEGORIES: readonly ThemeCategory[] = ['Default', 'Girls', 'Mens']

/** `settings.json` is the source of truth, but it only arrives after an
 *  `invoke` round trip — by then the window has already painted. This copy
 *  is read synchronously at startup so the first frame is already in the
 *  right theme instead of flashing the default one. */
const CACHE_KEY = 'rezure-appearance'
/** Where the title bar's light/dark toggle used to keep its choice, before
 *  appearance moved into `settings.json`. Migrated once, then removed. */
const LEGACY_THEME_KEY = 'rezure-theme'

/** The sliders on the Appearance page, all in percent. Mirrors the ranges
 *  in `config::settings`; Rust clamps anyway, these keep the sliders from
 *  offering values that would be clamped away. */
export const ADJUSTMENTS = {
  brightness: { min: 70, max: 120, step: 1, default: 100 },
  saturation: { min: 50, max: 150, step: 1, default: 100 },
  glassSolidity: { min: 0, max: 60, step: 1, default: 0 },
} as const

export type AdjustmentKey = keyof typeof ADJUSTMENTS

/** Webview zoom steps for "Interface size". */
export const UI_SCALES = [90, 100, 110, 125] as const

const DEFAULTS: AppearanceSettings = {
  mode: 'light',
  theme: 'rezure',
  showDecoration: true,
  brightness: ADJUSTMENTS.brightness.default,
  saturation: ADJUSTMENTS.saturation.default,
  glassSolidity: ADJUSTMENTS.glassSolidity.default,
  uiScale: 100,
  reduceMotion: false,
}

function inRange(value: unknown, min: number, max: number, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value)
    ? Math.min(max, Math.max(min, Math.round(value)))
    : fallback
}

function errorMessage(e: unknown): string {
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  return 'Something went wrong.'
}

function isMode(value: unknown): value is ThemeMode {
  return value === 'light' || value === 'dark' || value === 'system'
}

function isTheme(value: unknown): value is ThemePreset {
  return THEMES.some((t) => t.id === value)
}

/** Whatever is in localStorage, validated field by field — it is a cache a
 *  user (or an older build) could have left in any shape. */
function readCache(): AppearanceSettings {
  try {
    const raw = localStorage.getItem(CACHE_KEY)
    if (raw) {
      const parsed: unknown = JSON.parse(raw)
      if (parsed && typeof parsed === 'object') {
        const p = parsed as Record<string, unknown>
        return {
          mode: isMode(p.mode) ? p.mode : DEFAULTS.mode,
          theme: isTheme(p.theme) ? p.theme : DEFAULTS.theme,
          showDecoration:
            typeof p.showDecoration === 'boolean' ? p.showDecoration : DEFAULTS.showDecoration,
          brightness: inRange(p.brightness, 70, 120, DEFAULTS.brightness),
          saturation: inRange(p.saturation, 50, 150, DEFAULTS.saturation),
          glassSolidity: inRange(p.glassSolidity, 0, 60, DEFAULTS.glassSolidity),
          uiScale: inRange(p.uiScale, 80, 130, DEFAULTS.uiScale),
          reduceMotion:
            typeof p.reduceMotion === 'boolean' ? p.reduceMotion : DEFAULTS.reduceMotion,
        }
      }
    }
    const legacy = localStorage.getItem(LEGACY_THEME_KEY)
    if (legacy === 'light' || legacy === 'dark') return { ...DEFAULTS, mode: legacy }
  } catch {
    // localStorage unavailable or the cache is corrupt — fall through
  }
  return { ...DEFAULTS }
}

function writeCache(value: AppearanceSettings) {
  try {
    localStorage.setItem(CACHE_KEY, JSON.stringify(value))
  } catch {
    // ignore — the cache only saves a flash at startup
  }
}

export const useAppearanceStore = defineStore('appearance', () => {
  const initial = readCache()
  const mode = ref<ThemeMode>(initial.mode)
  const theme = ref<ThemePreset>(initial.theme)
  const showDecoration = ref(initial.showDecoration)
  const brightness = ref(initial.brightness)
  const saturation = ref(initial.saturation)
  const glassSolidity = ref(initial.glassSolidity)
  const uiScale = ref(initial.uiScale)
  const reduceMotion = ref(initial.reduceMotion)
  const error = ref<string | null>(null)

  // Tracks Windows' light/dark setting, used only while mode is "system".
  const systemQuery = window.matchMedia('(prefers-color-scheme: dark)')
  const systemDark = ref(systemQuery.matches)
  systemQuery.addEventListener('change', (e) => {
    systemDark.value = e.matches
  })

  const isDark = computed(() =>
    mode.value === 'system' ? systemDark.value : mode.value === 'dark',
  )

  const current = computed<AppearanceSettings>(() => ({
    mode: mode.value,
    theme: theme.value,
    showDecoration: showDecoration.value,
    brightness: brightness.value,
    saturation: saturation.value,
    glassSolidity: glassSolidity.value,
    uiScale: uiScale.value,
    reduceMotion: reduceMotion.value,
  }))

  const isAdjusted = computed(
    () =>
      brightness.value !== DEFAULTS.brightness ||
      saturation.value !== DEFAULTS.saturation ||
      glassSolidity.value !== DEFAULTS.glassSolidity ||
      uiScale.value !== DEFAULTS.uiScale ||
      reduceMotion.value !== DEFAULTS.reduceMotion,
  )

  // Runs immediately when the store is created (main.ts does that before
  // mounting), so the first paint already carries the cached choice.
  watchEffect(() => {
    const root = document.documentElement
    root.classList.toggle('dark', isDark.value)
    root.classList.toggle('no-decoration', !showDecoration.value)
    root.dataset.theme = theme.value
    root.classList.toggle('reduce-motion', reduceMotion.value)
    root.style.setProperty('--glass-solidity', String(glassSolidity.value / 100))
    // On <html> a filter doesn't turn the page into a containing block for
    // `position: fixed` (it would on any other element), so modals and the
    // sticker layer are unaffected. Left unset at 100 % so the default look
    // costs no extra compositing.
    const filters = [
      brightness.value !== 100 ? `brightness(${brightness.value}%)` : '',
      saturation.value !== 100 ? `saturate(${saturation.value}%)` : '',
    ].filter(Boolean)
    root.style.filter = filters.join(' ')
    writeCache(current.value)
  })

  // Real webview zoom rather than CSS `zoom`: with CSS zoom the full-height
  // layout (100vh) is scaled too and overflows the window.
  watchEffect(() => {
    const factor = uiScale.value / 100
    try {
      getCurrentWebview()
        .setZoom(factor)
        .catch(() => {
          // outside Tauri, or the permission is missing — keep 100 %
        })
    } catch {
      // not running inside Tauri
    }
  })

  function apply(value: AppearanceSettings) {
    mode.value = value.mode
    theme.value = value.theme
    showDecoration.value = value.showDecoration
    brightness.value = value.brightness
    saturation.value = value.saturation
    glassSolidity.value = value.glassSolidity
    uiScale.value = value.uiScale
    reduceMotion.value = value.reduceMotion
  }

  /** Loads the saved appearance. The first time this build runs there is
   *  none yet; the cached (or legacy title-bar) choice is saved instead, so
   *  updating Rezure doesn't reset someone's dark mode. */
  async function load() {
    try {
      const settings = await invoke<Settings>('get_settings')
      if (settings.appearance) {
        apply(settings.appearance)
      } else {
        await persist()
      }
      try {
        localStorage.removeItem(LEGACY_THEME_KEY)
      } catch {
        // ignore
      }
    } catch (e) {
      error.value = errorMessage(e)
    }
  }

  /** The new value is shown immediately; saving happens behind it. A failed
   *  save leaves the change on screen for this session and says so. The
   *  response is deliberately not applied back: with two quick clicks, the
   *  first one's reply would briefly undo the second. */
  async function persist() {
    error.value = null
    try {
      await invoke<Settings>('update_settings', { patch: { appearance: current.value } })
    } catch (e) {
      error.value = errorMessage(e)
    }
  }

  function setMode(value: ThemeMode) {
    mode.value = value
    return persist()
  }

  function setTheme(value: ThemePreset) {
    theme.value = value
    return persist()
  }

  function setShowDecoration(value: boolean) {
    showDecoration.value = value
    return persist()
  }

  const adjustmentRefs = { brightness, saturation, glassSolidity }

  /** Sliders: `save` false while dragging (on screen only), then once with
   *  `save` true when let go. */
  function setAdjustment(key: AdjustmentKey, value: number, save = true) {
    const { min, max } = ADJUSTMENTS[key]
    adjustmentRefs[key].value = Math.min(max, Math.max(min, Math.round(value)))
    return save ? persist() : Promise.resolve()
  }

  function setUiScale(value: number) {
    uiScale.value = value
    return persist()
  }

  function setReduceMotion(value: boolean) {
    reduceMotion.value = value
    return persist()
  }

  /** Puts every slider and switch under "Adjustments" back; leaves the
   *  theme, decoration and light/dark alone. */
  function resetAdjustments() {
    brightness.value = DEFAULTS.brightness
    saturation.value = DEFAULTS.saturation
    glassSolidity.value = DEFAULTS.glassSolidity
    uiScale.value = DEFAULTS.uiScale
    reduceMotion.value = DEFAULTS.reduceMotion
    return persist()
  }

  /** The title bar shortcut: flips what is on screen. From "system" that
   *  means picking the opposite of what Windows currently shows. */
  function toggleDark() {
    return setMode(isDark.value ? 'light' : 'dark')
  }

  return {
    mode,
    theme,
    showDecoration,
    brightness,
    saturation,
    glassSolidity,
    uiScale,
    reduceMotion,
    isAdjusted,
    isDark,
    error,
    load,
    setMode,
    setTheme,
    setShowDecoration,
    setAdjustment,
    setUiScale,
    setReduceMotion,
    resetAdjustments,
    persist,
    toggleDark,
  }
})
