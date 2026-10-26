<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { useProjectsStore } from '@/stores/projects'
import type { FrameworkSummary } from '@/types/project'

/**
 * The open list of `FrameworkPicker`: a search box, "Auto", every framework
 * under its category heading, and "Custom folder…". Keys are `auto`,
 * `fw:<id>` and `custom`; the picker turns them into a choice.
 */
const props = defineProps<{
  /** The trigger the menu hangs from. */
  anchor: HTMLElement | null
  selectedKey: string
  /** The framework id detection found, for the "Auto" row. */
  detected: string
}>()

const emit = defineEmits<{
  select: [key: string]
  /** `refocus` when closed from the keyboard, so focus goes back to the
   *  trigger rather than being lost; not after a click elsewhere. */
  close: [refocus: boolean]
}>()

const store = useProjectsStore()

const panel = ref<HTMLElement | null>(null)
const list = ref<HTMLElement | null>(null)
const searchInput = ref<HTMLInputElement | null>(null)
const query = ref('')

interface Option {
  key: string
  label: string
  detail: string | null
  docroot: string | null
  /** Served through its own dev server rather than from a folder. */
  devServer: boolean
  own: boolean
}

type Row =
  | { type: 'option'; option: Option; index: number }
  | { type: 'heading'; text: string }
  | { type: 'divider' }
  | { type: 'empty' }

const autoOption = computed<Option>(() => ({
  key: 'auto',
  label: 'Auto',
  detail:
    props.detected === 'unknown'
      ? 'No framework markers found'
      : `Detected: ${store.frameworkLabel(props.detected)}`,
  docroot: store.frameworkCatalog.frameworks.find((f) => f.id === props.detected)?.docroot ?? null,
  devServer:
    store.frameworkCatalog.frameworks.find((f) => f.id === props.detected)?.serve === 'proxy',
  own: false,
}))

const CUSTOM_OPTION: Option = {
  key: 'custom',
  label: 'Custom folder…',
  detail: 'Serve a subfolder you name',
  docroot: null,
  devServer: false,
  own: false,
}

function matches(framework: FrameworkSummary, q: string) {
  return (
    framework.label.toLowerCase().includes(q) ||
    framework.id.includes(q) ||
    framework.category.toLowerCase().includes(q)
  )
}

const frameworks = computed(() => {
  const q = query.value.trim().toLowerCase()
  return store.frameworkCatalog.frameworks.filter((f) => !q || matches(f, q))
})

/** Every row in display order, headings between categories. The catalog
 *  arrives ordered by category, so a heading goes wherever it changes. */
const rows = computed<Row[]>(() => {
  const result: Row[] = []
  let index = 0
  const add = (option: Option) => result.push({ type: 'option', option, index: index++ })

  add(autoOption.value)
  let category: string | null = null
  for (const framework of frameworks.value) {
    if (framework.category !== category) {
      category = framework.category
      result.push({ type: 'heading', text: category })
    }
    add({
      key: `fw:${framework.id}`,
      label: framework.label,
      detail: null,
      docroot: framework.docroot,
      devServer: framework.serve === 'proxy',
      own: framework.source === 'user',
    })
  }
  if (frameworks.value.length === 0) result.push({ type: 'empty' })
  result.push({ type: 'divider' })
  add(CUSTOM_OPTION)
  return result
})

/** Just the selectable rows, in order — what the arrow keys walk. */
const options = computed<Option[]>(() =>
  rows.value.flatMap((row) => (row.type === 'option' ? [row.option] : [])),
)

/** The row the keyboard (or the pointer) is on. */
const active = ref(0)

function scrollActiveIntoView() {
  nextTick(() => {
    list.value
      ?.querySelector(`[data-index="${active.value}"]`)
      ?.scrollIntoView({ block: 'nearest' })
  })
}

// Typing narrows the list; start from its first framework, not "Auto".
watch(query, () => {
  active.value = frameworks.value.length > 0 ? 1 : 0
  scrollActiveIntoView()
})

function move(step: number) {
  const count = options.value.length
  active.value = (active.value + step + count) % count
  scrollActiveIntoView()
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'ArrowDown') {
    e.preventDefault()
    move(1)
  } else if (e.key === 'ArrowUp') {
    e.preventDefault()
    move(-1)
  } else if (e.key === 'Enter') {
    e.preventDefault()
    const option = options.value[active.value]
    if (option) emit('select', option.key)
  } else if (e.key === 'Escape') {
    e.preventDefault()
    e.stopPropagation()
    emit('close', true)
  } else if (e.key === 'Tab') {
    emit('close', false)
  }
}

const GAP = 6
const MAX_HEIGHT = 380
/** Below this much room under the trigger, the menu opens upward instead. */
const MIN_HEIGHT = 220

const position = ref({ top: 'auto', bottom: 'auto', left: 0, width: 0, maxHeight: MAX_HEIGHT })

/**
 * Rendered into `<body>` and placed against the trigger's viewport rect, the
 * same way `OpenInClientMenu` does it: inside a modal the list would
 * otherwise be cut off at the dialog's edge, or run off the bottom of the
 * window.
 */
function place() {
  const rect = props.anchor?.getBoundingClientRect()
  if (!rect) return
  const below = window.innerHeight - rect.bottom - GAP - 8
  const above = rect.top - GAP - 8
  const flipUp = below < MIN_HEIGHT && above > below
  position.value = {
    top: flipUp ? 'auto' : `${rect.bottom + GAP}px`,
    bottom: flipUp ? `${window.innerHeight - rect.top + GAP}px` : 'auto',
    left: rect.left,
    width: rect.width,
    maxHeight: Math.min(MAX_HEIGHT, flipUp ? above : below),
  }
}

function onDocumentPointerDown(e: PointerEvent) {
  const target = e.target as Node
  // The trigger toggles the menu itself; treating its click as "outside"
  // would close and immediately reopen it.
  if (panel.value?.contains(target) || props.anchor?.contains(target)) return
  emit('close', false)
}

/** A fixed-position menu doesn't travel with the page, so scrolling
 *  anything but the list itself closes it rather than leaving it stranded. */
function onScroll(e: Event) {
  if (panel.value?.contains(e.target as Node)) return
  emit('close', false)
}

function onResize() {
  emit('close', false)
}

onMounted(() => {
  place()
  active.value = Math.max(
    0,
    options.value.findIndex((o) => o.key === props.selectedKey),
  )
  nextTick(() => {
    searchInput.value?.focus()
    scrollActiveIntoView()
  })
  document.addEventListener('pointerdown', onDocumentPointerDown)
  window.addEventListener('scroll', onScroll, true)
  window.addEventListener('resize', onResize)
})

onUnmounted(() => {
  document.removeEventListener('pointerdown', onDocumentPointerDown)
  window.removeEventListener('scroll', onScroll, true)
  window.removeEventListener('resize', onResize)
})
</script>

<template>
  <Teleport to="body">
    <Transition
      appear
      enter-active-class="transition duration-100 ease-out"
      enter-from-class="opacity-0 scale-[0.98]"
    >
      <div
        ref="panel"
        class="glass-strong fixed z-60 flex flex-col overflow-hidden rounded-2xl"
        :style="{
          top: position.top,
          bottom: position.bottom,
          left: `${position.left}px`,
          width: `${position.width}px`,
          maxHeight: `${position.maxHeight}px`,
        }"
      >
        <div class="glass-divider flex shrink-0 items-center gap-2 border-b px-3.5 py-2.5">
          <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            class="h-4 w-4 shrink-0 text-neutral-400"
          >
            <circle cx="11" cy="11" r="6.5" />
            <path stroke-linecap="round" d="m20 20-4.2-4.2" />
          </svg>
          <input
            ref="searchInput"
            v-model="query"
            type="text"
            placeholder="Search frameworks"
            aria-label="Search frameworks"
            class="w-full bg-transparent text-sm text-neutral-900 outline-none placeholder:text-neutral-400 dark:text-neutral-100"
            @keydown="onKeydown"
          />
        </div>

        <div ref="list" role="listbox" class="min-h-0 flex-1 overflow-y-auto p-1.5">
          <template v-for="(row, i) in rows" :key="i">
            <p
              v-if="row.type === 'heading'"
              class="px-2.5 pt-3 pb-1 text-[11px] font-semibold tracking-wide text-neutral-400 uppercase"
            >
              {{ row.text }}
            </p>
            <div v-else-if="row.type === 'divider'" class="glass-divider mx-1 my-1.5 border-t" />
            <p v-else-if="row.type === 'empty'" class="px-2.5 py-3 text-sm text-neutral-500">
              No framework matches “{{ query.trim() }}”.
            </p>
            <button
              v-else
              type="button"
              role="option"
              :aria-selected="row.option.key === selectedKey"
              :data-index="row.index"
              class="flex w-full items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-sm transition-colors"
              :class="row.index === active ? 'bg-white/70 dark:bg-white/8' : ''"
              @mousemove="active = row.index"
              @click="emit('select', row.option.key)"
            >
              <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2.5"
                class="h-4 w-4 shrink-0 text-accent-600 dark:text-accent-400"
                :class="row.option.key === selectedKey ? '' : 'invisible'"
              >
                <path stroke-linecap="round" stroke-linejoin="round" d="m5 12.5 4.5 4.5L19 7.5" />
              </svg>
              <span class="min-w-0 flex-1">
                <span
                  class="block truncate font-medium"
                  :class="
                    row.option.key === selectedKey
                      ? 'text-accent-600 dark:text-accent-400'
                      : 'text-neutral-800 dark:text-neutral-100'
                  "
                >
                  {{ row.option.label }}
                </span>
                <span v-if="row.option.detail" class="block truncate text-xs text-neutral-500">
                  {{ row.option.detail }}
                </span>
              </span>
              <span
                v-if="row.option.own"
                class="glass-inset shrink-0 rounded-full px-1.5 py-0.5 text-[10px] font-semibold tracking-wide text-neutral-500 uppercase dark:text-neutral-400"
              >
                Yours
              </span>
              <span
                v-if="row.option.devServer"
                class="glass-inset shrink-0 rounded-md px-1.5 py-0.5 text-[11px] text-neutral-500 dark:text-neutral-400"
                title="Served through its own dev server, which Rezure can start"
              >
                dev server
              </span>
              <span
                v-else-if="row.option.docroot"
                class="glass-inset shrink-0 rounded-md px-1.5 py-0.5 font-mono text-[11px] text-neutral-500 dark:text-neutral-400"
                :title="`Served from ${row.option.docroot}/`"
              >
                {{ row.option.docroot }}/
              </span>
            </button>
          </template>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>
