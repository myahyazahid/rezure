import { computed, onUnmounted, ref, type CSSProperties, type Ref } from 'vue'

export interface ColumnSpec {
  /** The column's share of the width until the user drags one. Columns
   *  split the room in proportion to it, so the table fits its container. */
  initial: number
  /** Narrowest it can be squeezed or dragged to. */
  min: number
}

/**
 * Column widths for a table whose headings can be dragged to resize.
 *
 * Until the user drags a heading, the columns share the container's width in
 * proportion to `initial`, so the table always fits. The first drag fixes
 * every column at the width it has on screen, then changes the dragged one;
 * from then on the widths are the user's and are remembered, and a table
 * wider than its container scrolls sideways. Double-clicking a handle goes
 * back to fitting.
 *
 * Remembered in localStorage rather than the app config: they're a
 * per-machine convenience, and losing them only means the table fits again.
 */
export function useResizableColumns<K extends string>(
  storageKey: string,
  columns: Record<K, ColumnSpec>,
) {
  const keys = Object.keys(columns) as K[]

  /** The user's widths, or `null` while the table fits its container. All
   *  or nothing, so a fixed column never sits next to a stretching one. */
  function load(): Record<K, number> | null {
    try {
      const raw = localStorage.getItem(storageKey)
      if (!raw) return null
      const parsed: unknown = JSON.parse(raw)
      if (typeof parsed !== 'object' || parsed === null) return null
      const out = {} as Record<K, number>
      for (const key of keys) {
        const value = (parsed as Record<string, unknown>)[key]
        if (typeof value !== 'number' || !Number.isFinite(value)) return null
        out[key] = Math.max(columns[key].min, Math.round(value))
      }
      return out
    } catch {
      // Unavailable or corrupt — fitting is fine.
      return null
    }
  }

  function save() {
    try {
      if (widths.value === null) localStorage.removeItem(storageKey)
      else localStorage.setItem(storageKey, JSON.stringify(widths.value))
    } catch {
      // Best-effort — a blocked localStorage only means widths aren't remembered.
    }
  }

  const widths: Ref<Record<K, number> | null> = ref(load()) as Ref<Record<K, number> | null>
  const resizing: Ref<K | null> = ref(null)

  /** True while the columns fit the container, false once the user has set
   *  widths. With set widths the table needs a spacer to take what's left. */
  const fitting = computed(() => widths.value === null)

  /** Inline style for a column's heading and its cells. A fitting column
   *  contains its inline size, so a long (truncated) value doesn't count
   *  towards the table's `max-content` width and push it into scrolling. */
  function styleFor(key: K): CSSProperties {
    const fixed = widths.value?.[key]
    return fixed === undefined
      ? {
          flex: `${columns[key].initial} 1 0%`,
          minWidth: `${columns[key].min}px`,
          contain: 'inline-size',
        }
      : { width: `${fixed}px`, flexShrink: 0 }
  }

  let startX = 0
  let startWidth = 0

  function onMove(event: PointerEvent) {
    const key = resizing.value
    if (key === null || widths.value === null) return
    const next = startWidth + event.clientX - startX
    widths.value[key] = Math.max(columns[key].min, Math.round(next))
  }

  function stop() {
    if (resizing.value === null) return
    resizing.value = null
    document.body.style.cursor = ''
    document.body.style.userSelect = ''
    window.removeEventListener('pointermove', onMove)
    window.removeEventListener('pointerup', stop)
    window.removeEventListener('pointercancel', stop)
    save()
  }

  /** The widths the headings have on screen — what fixing them keeps. Each
   *  heading carries `data-column="<key>"`, and they share a parent. */
  function measure(heading: Element | null | undefined): Record<K, number> {
    const out = {} as Record<K, number>
    for (const key of keys) {
      const el = heading?.parentElement?.querySelector(`[data-column="${key}"]`)
      const width = el?.getBoundingClientRect().width ?? 0
      out[key] = Math.max(columns[key].min, Math.round(width))
    }
    return out
  }

  /** Bind to `pointerdown` on a column's drag handle, placed inside the
   *  column's heading. */
  function startResize(key: K, event: PointerEvent) {
    if (event.button !== 0) return
    event.preventDefault()
    if (widths.value === null) {
      widths.value = measure((event.currentTarget as HTMLElement | null)?.parentElement)
    }
    resizing.value = key
    startX = event.clientX
    startWidth = widths.value[key]
    // On the body, so the cursor and the no-select hold while the pointer
    // wanders off the narrow handle during the drag.
    document.body.style.cursor = 'col-resize'
    document.body.style.userSelect = 'none'
    window.addEventListener('pointermove', onMove)
    window.addEventListener('pointerup', stop)
    window.addEventListener('pointercancel', stop)
  }

  /** Back to fitting the container — bound to double-clicking a handle. */
  function reset() {
    widths.value = null
    save()
  }

  onUnmounted(stop)

  return { resizing, fitting, styleFor, startResize, reset }
}
