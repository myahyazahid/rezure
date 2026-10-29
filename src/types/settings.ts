export type DomainSuffix = 'test' | 'local' | 'dev'

export type ThemeMode = 'light' | 'dark' | 'system'

/** Mirrors `config::settings::ThemePreset`; the colours live in `main.css`. */
export type ThemePreset =
  | 'rezure'
  | 'fullglass'
  | 'clearglass'
  | 'skyglass'
  | 'blossom'
  | 'softpink'
  | 'lavender'
  | 'peach'
  | 'matcha'
  | 'midnight'
  | 'navy'
  | 'carbon'
  | 'forest'
  | 'terminal'

export interface AppearanceSettings {
  mode: ThemeMode
  theme: ThemePreset
  showDecoration: boolean
  /** Percent, 70–120. */
  brightness: number
  /** Percent, 50–150. */
  saturation: number
  /** Percent of solid colour over the glass, 0–60. */
  glassSolidity: number
  /** Webview zoom in percent, 80–130. */
  uiScale: number
  reduceMotion: boolean
}

/** Mirrors `config::stickers::StickerKind`; art in `src/assets/stickers/<kind>.svg`
 *  (Classic) and `src/assets/stickers/live/<kind>.svg` (Live, animated). */
export type StickerKind = ClassicStickerKind | LiveStickerKind

export type ClassicStickerKind =
  | 'bow'
  | 'heart'
  | 'sparkle'
  | 'star'
  | 'sakura'
  | 'cloud'
  | 'strawberry'
  | 'cat'
  | 'butterfly'
  | 'rainbow'
  | 'crown'
  | 'cherry'
  | 'gamepad'
  | 'rocket'
  | 'bolt'
  | 'flame'
  | 'coffee'
  | 'terminal'
  | 'football'
  | 'headphones'
  | 'shield'
  | 'robot'
  | 'planet'
  | 'sunglasses'

export type LiveStickerKind =
  | 'baby-chick'
  | 'baby-panda'
  | 'baby-frog'
  | 'baby-kitten'
  | 'baby-hedgehog'
  | 'baby-hamster'
  | 'baby-penguin'
  | 'baby-bunny'
  | 'baby-lion'
  | 'baby-trex'
  | 'baby-duck'
  | 'baby-capybara'
  | 'baby-bear'
  | 'baby-piglet'
  | 'baby-puppy'
  | 'baby-seal'
  | 'baby-axolotl'
  | 'baby-koala'
  | 'baby-otter'
  | 'baby-sloth'
  | 'baby-whale'
  | 'baby-kitsune'

/** A sticker downloaded from Decorations → Browse: `saved:` plus its catalog id.
 *  Mirrors `config::stickers::StickerArt::Saved`. */
export type SavedStickerKind = `saved:${string}`

/** What a placed sticker shows — art that ships with the app, or a download. */
export type StickerArt = StickerKind | SavedStickerKind

/** A placed sticker. Position and size are percentages of the window, so the
 *  Decorations preview and the real window agree at any size. */
export interface Sticker {
  id: string
  kind: StickerArt
  /** Centre, 0–100 % of the window. */
  x: number
  y: number
  /** Width, % of the window's width. */
  size: number
  /** Degrees, -180–180. */
  rotation: number
  flip: boolean
}

export interface Decorations {
  visible: boolean
  /** Whether Live stickers move (they are also still while Reduce motion is on). */
  animate: boolean
  /** Paint order: later entries are drawn on top. */
  stickers: Sticker[]
}

export interface Settings {
  defaultPort: number
  shareUsageData: boolean
  activePhpVersion: string | null
  activeNodeVersion: string | null
  activePostgresVersion: string | null
  startWithWindows: boolean
  keepInTrayOnClose: boolean
  notifyOnCrash: boolean
  autoWriteHosts: boolean
  /** `null` until an appearance has been saved — see `stores/appearance.ts`. */
  appearance: AppearanceSettings | null
  decorations: Decorations
  /** Ids removed from the Services page with Manage services. */
  hiddenServices: string[]
}

export interface SettingsPatch {
  defaultPort?: number
  shareUsageData?: boolean
  startWithWindows?: boolean
  keepInTrayOnClose?: boolean
  notifyOnCrash?: boolean
  autoWriteHosts?: boolean
  appearance?: AppearanceSettings
  decorations?: Decorations
}

/** Where Rezure's own state lives on disk — read-only, shown so it can be
 *  found without digging through docs. */
export interface StoragePaths {
  wwwRoot: string
  binariesDir: string
  dropInDir: string
  dumpsDir: string
}
