/** Mirrors `services::sticker_catalog::Format`. */
export type StickerFormat = 'svg' | 'png' | 'webp'

/** One sticker the catalog offers — `services::sticker_catalog::CatalogEntry`. */
export interface CatalogSticker {
  /** A slug; the id a downloaded copy is saved under. */
  id: string
  name: string
  /** A slug (`girls`, `pixel-art`…). Free-form: a new one simply appears. */
  category: string
  format: StickerFormat
  /** Bytes. */
  size: number
  /** Lowercase hex SHA-256 — changes when the server replaces the image. */
  sha256: string
  /** Where to show a preview from before it's downloaded. */
  previewUrl: string
}

/** What the catalog fetch returns — `services::sticker_catalog::Catalog`. */
export interface StickerCatalog {
  stickers: CatalogSticker[]
  /** The list is the local cache because the server couldn't be reached. */
  offline: boolean
  error: string | null
}

/** A downloaded sticker with its image inline — `SavedStickerView`. */
export interface SavedSticker {
  id: string
  name: string
  category: string
  format: StickerFormat
  size: number
  /** What it was verified against; differs from the catalog's when the
   *  server has since replaced the image. */
  sha256: string
  /** `data:image/…;base64,…` — drawn as an `<img>` source. */
  dataUrl: string
}

/** Where a catalog sticker stands against the downloads. */
export type DownloadState = 'new' | 'saved' | 'outdated'
