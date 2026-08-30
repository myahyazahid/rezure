<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useDatabasesStore } from '@/stores/databases'

const props = defineProps<{ database: string }>()

const store = useDatabasesStore()

const open = ref(false)
const root = ref<HTMLElement | null>(null)
const button = ref<HTMLElement | null>(null)

/** With exactly one client there's nothing to choose between, so the button
 *  opens it directly instead of showing a one-item menu. */
const single = computed(() => (store.clients.length === 1 ? store.clients[0] : null))

const MENU_WIDTH = 240
const MENU_GAP = 6
/** Roughly a header plus five entries — only used to decide whether the menu
 *  has room below the button or has to flip above it. */
const MENU_MAX_HEIGHT = 260

/** Anchored by `top` normally, by `bottom` when flipped above the button —
 *  anchoring a flipped menu by `top` would need its real height, which isn't
 *  known until after it renders. */
const position = ref<{ top: string; bottom: string; left: number }>({
  top: 'auto',
  bottom: 'auto',
  left: 0,
})

/**
 * The menu is rendered into `<body>` rather than next to its button.
 *
 * The row lives inside the table card, which is `overflow-hidden` to keep
 * its rounded corners — an absolutely-positioned menu inside it gets
 * clipped at the card's edge instead of overlapping the page. Teleporting
 * out and positioning against the button's viewport rect sidesteps that
 * (and every future ancestor that clips or creates a stacking context).
 */
function place() {
  const rect = button.value?.getBoundingClientRect()
  if (!rect) return

  const flipUp =
    rect.bottom + MENU_GAP + MENU_MAX_HEIGHT > window.innerHeight && rect.top > MENU_MAX_HEIGHT
  position.value = {
    top: flipUp ? 'auto' : `${rect.bottom + MENU_GAP}px`,
    bottom: flipUp ? `${window.innerHeight - rect.top + MENU_GAP}px` : 'auto',
    // Right-aligned with the button, then kept inside the viewport.
    left: Math.max(8, Math.min(rect.right - MENU_WIDTH, window.innerWidth - MENU_WIDTH - 8)),
  }
}

function activate(clientId: string) {
  open.value = false
  store.openInClient(clientId, props.database)
}

function onClick() {
  if (single.value) {
    activate(single.value.id)
    return
  }
  if (!open.value) place()
  open.value = !open.value
}

function onDocumentPointerDown(e: PointerEvent) {
  const target = e.target as Node
  if (root.value?.contains(target)) return
  // The menu itself is outside `root` now that it's teleported, so its own
  // clicks would otherwise read as "outside" and close it before they land.
  if ((target as HTMLElement).closest?.('[data-open-with-menu]')) return
  open.value = false
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') open.value = false
}

/** A fixed-position menu doesn't travel with the page, so scrolling closes
 *  it rather than leaving it stranded over unrelated rows. */
function onScrollOrResize() {
  if (open.value) open.value = false
}

onMounted(() => {
  document.addEventListener('pointerdown', onDocumentPointerDown)
  window.addEventListener('keydown', onKeydown)
  window.addEventListener('scroll', onScrollOrResize, true)
  window.addEventListener('resize', onScrollOrResize)
})
onUnmounted(() => {
  document.removeEventListener('pointerdown', onDocumentPointerDown)
  window.removeEventListener('keydown', onKeydown)
  window.removeEventListener('scroll', onScrollOrResize, true)
  window.removeEventListener('resize', onScrollOrResize)
})
</script>

<template>
  <div ref="root" class="relative">
    <button
      ref="button"
      type="button"
      class="flex h-9 shrink-0 items-center gap-1.5 rounded-full bg-red-600 px-3.5 text-sm font-semibold text-white transition select-none hover:bg-red-500"
      :title="
        single
          ? `Open ${props.database} in ${single.name}`
          : `Open ${props.database} in a SQL client`
      "
      @click="onClick"
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="h-4 w-4">
        <path stroke-linecap="round" stroke-linejoin="round" d="M8 16 16 8M9 8h7v7" />
      </svg>
      Open
    </button>

    <Teleport to="body">
      <div
        v-if="open"
        data-open-with-menu
        class="fixed z-50 overflow-hidden rounded-2xl border border-neutral-200 bg-white shadow-xl dark:border-neutral-700 dark:bg-neutral-900"
        :style="{
          top: position.top,
          bottom: position.bottom,
          left: `${position.left}px`,
          width: `${MENU_WIDTH}px`,
        }"
      >
        <p
          class="border-b border-neutral-200/70 px-4 py-2 text-[11px] font-semibold tracking-wide text-neutral-400 uppercase dark:border-neutral-800"
        >
          Open {{ props.database }} with
        </p>
        <button
          v-for="client in store.clients"
          :key="client.id"
          type="button"
          class="flex w-full items-center justify-between gap-2 px-4 py-2.5 text-left text-sm transition hover:bg-neutral-100 dark:hover:bg-neutral-800"
          @click="activate(client.id)"
        >
          <span class="truncate font-medium text-neutral-800 dark:text-neutral-100">
            {{ client.name }}
          </span>
          <!-- Workbench and HeidiSQL can only be pointed at a server, so the
               menu says so rather than letting the click look like it failed. -->
          <span v-if="!client.opensDatabase" class="shrink-0 text-[11px] text-neutral-400">
            server only
          </span>
        </button>
      </div>
    </Teleport>
  </div>
</template>
