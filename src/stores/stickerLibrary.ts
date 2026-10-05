import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type {
  CatalogSticker,
  DownloadState,
  SavedSticker,
  StickerCatalog,
} from '@/types/stickerLibrary'

function errorMessage(e: unknown): string {
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  return 'Something went wrong.'
}

/**
 * The stickers on offer in Decorations → Browse, and the ones already
 * downloaded.
 *
 * Only state and calls: what makes a download acceptable (its checksum, its
 * format, its size) is decided in Rust, which also refuses to keep one that
 * fails. Nothing here can mark a sticker as saved that the backend didn't
 * save.
 */
export const useStickerLibraryStore = defineStore('stickerLibrary', () => {
  const saved = ref<SavedSticker[]>([])
  /** Whether the saved list has been read successfully. Nothing is pruned
   *  from the placed stickers before it has — an unread list isn't an empty
   *  one. */
  const savedLoaded = ref(false)

  const catalog = ref<CatalogSticker[]>([])
  const catalogLoaded = ref(false)
  const loadingCatalog = ref(false)
  /** The catalog is the local cache because the server couldn't be reached. */
  const offline = ref(false)
  const catalogError = ref<string | null>(null)

  /** Ids with a download in flight. */
  const busy = ref<string[]>([])
  const error = ref<string | null>(null)

  const savedById = computed(() => new Map(saved.value.map((s) => [s.id, s])))
  const categories = computed(() => [...new Set(catalog.value.map((s) => s.category))].sort())

  function isBusy(id: string): boolean {
    return busy.value.includes(id)
  }

  /** The image of a downloaded sticker, or `''` when it isn't one (or isn't
   *  loaded yet) — callers skip drawing an empty source. */
  function urlFor(id: string): string {
    return savedById.value.get(id)?.dataUrl ?? ''
  }

  function nameFor(id: string): string | null {
    return savedById.value.get(id)?.name ?? null
  }

  /** `outdated` when the server's image has changed since it was downloaded. */
  function stateOf(sticker: CatalogSticker): DownloadState {
    const have = savedById.value.get(sticker.id)
    if (!have) return 'new'
    return have.sha256 === sticker.sha256 ? 'saved' : 'outdated'
  }

  async function loadSaved(): Promise<boolean> {
    try {
      saved.value = await invoke<SavedSticker[]>('list_saved_stickers')
      savedLoaded.value = true
      return true
    } catch (e) {
      error.value = errorMessage(e)
      return false
    }
  }

  /** Never throws: an unreachable server comes back as the cached list. */
  async function fetchCatalog() {
    loadingCatalog.value = true
    try {
      const result = await invoke<StickerCatalog>('fetch_sticker_catalog')
      catalog.value = result.stickers
      offline.value = result.offline
      catalogError.value = result.error
      catalogLoaded.value = true
    } catch (e) {
      offline.value = true
      catalogError.value = errorMessage(e)
    } finally {
      loadingCatalog.value = false
    }
  }

  /** Downloads and saves one sticker (or replaces it with the server's newer
   *  image). Returns whether it was kept. */
  async function download(id: string): Promise<boolean> {
    if (isBusy(id)) return false
    error.value = null
    busy.value = [...busy.value, id]
    try {
      const downloaded = await invoke<SavedSticker>('download_sticker', { id })
      saved.value = [...saved.value.filter((s) => s.id !== id), downloaded]
      return true
    } catch (e) {
      error.value = errorMessage(e)
      return false
    } finally {
      busy.value = busy.value.filter((b) => b !== id)
    }
  }

  /** Deletes the download. Copies placed on the window are the decorations
   *  store's to clear — use its `removeSaved`, which calls this. */
  async function remove(id: string): Promise<boolean> {
    error.value = null
    try {
      await invoke('remove_saved_sticker', { id })
      saved.value = saved.value.filter((s) => s.id !== id)
      return true
    } catch (e) {
      error.value = errorMessage(e)
      return false
    }
  }

  return {
    saved,
    savedLoaded,
    catalog,
    catalogLoaded,
    loadingCatalog,
    offline,
    catalogError,
    error,
    categories,
    isBusy,
    urlFor,
    nameFor,
    stateOf,
    loadSaved,
    fetchCatalog,
    download,
    remove,
  }
})
