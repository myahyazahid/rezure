<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue'
import { useProjectsStore } from '@/stores/projects'
import FrameworkPickerMenu from '@/components/projects/FrameworkPickerMenu.vue'
import type { FrameworkChoice } from '@/types/project'

/**
 * Picks what a project is served as: "Auto" (whatever detection says), any
 * framework in the registry, or a folder of the user's own ("Custom").
 *
 * Shared by the Add folder dialog and the per-project framework dialog, so
 * both offer exactly the same choices. The list comes from Rust
 * (`list_frameworks`) — nothing here knows which frameworks exist.
 *
 * Not a native `<select>`: Windows paints a select's option list itself,
 * outside the app's styling, and it can't carry a docroot or a heading.
 */
const props = defineProps<{
  modelValue: FrameworkChoice
  /** The framework id detection found, for "Auto". */
  detected: string
  disabled?: boolean
}>()

const emit = defineEmits<{
  'update:modelValue': [FrameworkChoice]
  /** True while "Custom folder…" is picked with no folder typed: the model
   *  still holds the previous choice, so the dialog shouldn't submit it. */
  pending: [boolean]
}>()

const store = useProjectsStore()

const open = ref(false)
const trigger = ref<HTMLButtonElement | null>(null)
const customInput = ref<HTMLInputElement | null>(null)
const customFolder = ref('')
/** "Custom folder…" was picked but no folder typed yet — nothing to apply,
 *  so the model still holds the previous choice meanwhile. */
const customPending = ref(false)

watch(
  () => props.modelValue,
  (choice) => {
    if (choice && 'docroot' in choice) customFolder.value = choice.docroot
    if (choice) customPending.value = false
  },
  { immediate: true },
)

watch(customPending, (pending) => emit('pending', pending))

const selectedKey = computed(() => {
  const choice = props.modelValue
  if (customPending.value) return 'custom'
  if (choice === null) return 'auto'
  return 'framework' in choice ? `fw:${choice.framework}` : 'custom'
})

function catalogEntry(id: string) {
  return store.frameworkCatalog.frameworks.find((f) => f.id === id) ?? null
}

/** What the closed picker shows: the framework, how it got there, and the
 *  folder it serves. */
const current = computed(() => {
  const key = selectedKey.value
  if (key === 'custom') {
    const folder = customFolder.value.trim()
    return {
      title: 'Custom folder',
      detail: 'Serves a subfolder you name',
      docroot: folder || null,
      devServer: false,
    }
  }
  if (key === 'auto') {
    const unknown = props.detected === 'unknown'
    return {
      title: store.frameworkLabel(props.detected),
      detail: unknown
        ? 'Auto · no framework markers found'
        : "Auto · detected from the project's files",
      docroot: catalogEntry(props.detected)?.docroot ?? null,
      devServer: catalogEntry(props.detected)?.serve === 'proxy',
    }
  }
  const id = key.slice(3)
  const entry = catalogEntry(id)
  return {
    title: store.frameworkLabel(id),
    detail: entry?.source === 'user' ? 'Chosen by you · your own definition' : 'Chosen by you',
    docroot: entry?.docroot ?? null,
    devServer: entry?.serve === 'proxy',
  }
})

function folderPill(folder: string) {
  return `${folder.replace(/\\/g, '/').replace(/\/+$/, '')}/`
}

function toggle() {
  if (!props.disabled) open.value = !open.value
}

function onTriggerKeydown(e: KeyboardEvent) {
  if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
    e.preventDefault()
    open.value = true
  }
}

function onMenuClose(refocus: boolean) {
  open.value = false
  if (refocus) trigger.value?.focus()
}

function onSelect(key: string) {
  open.value = false
  if (key === 'auto') {
    customPending.value = false
    emit('update:modelValue', null)
    trigger.value?.focus()
  } else if (key.startsWith('fw:')) {
    customPending.value = false
    emit('update:modelValue', { framework: key.slice(3) })
    trigger.value?.focus()
  } else {
    // Nothing to apply until there's a folder: an empty Custom would just
    // be refused.
    customPending.value = customFolder.value.trim() === ''
    emitCustom()
    nextTick(() => customInput.value?.focus())
  }
}

/** Sent on change (blur or Enter), not per keystroke: every partial folder
 *  name typed on the way would otherwise come back as "no such folder". */
function emitCustom() {
  const folder = customFolder.value.trim()
  if (!folder) return
  customPending.value = false
  emit('update:modelValue', { docroot: folder })
}
</script>

<template>
  <div>
    <button
      ref="trigger"
      type="button"
      aria-haspopup="listbox"
      :aria-expanded="open"
      :disabled="disabled"
      class="glass-inset flex w-full items-center gap-3 rounded-xl px-3.5 py-2.5 text-left transition-colors hover:border-accent-400/50 disabled:opacity-60"
      :class="open ? 'border-accent-400/70' : ''"
      @click="toggle"
      @keydown="onTriggerKeydown"
    >
      <span class="min-w-0 flex-1">
        <span class="block truncate text-sm font-semibold text-neutral-900 dark:text-neutral-100">
          {{ current.title }}
        </span>
        <span class="block truncate text-xs text-neutral-500">{{ current.detail }}</span>
      </span>
      <span
        v-if="current.devServer"
        class="glass-inset shrink-0 rounded-md px-1.5 py-0.5 text-[11px] text-neutral-500 dark:text-neutral-400"
        title="Served through its own dev server, which Rezure can start"
      >
        dev server
      </span>
      <span
        v-else-if="current.docroot"
        class="glass-inset shrink-0 rounded-md px-1.5 py-0.5 font-mono text-[11px] text-neutral-500 dark:text-neutral-400"
        :title="`Served from ${folderPill(current.docroot)}`"
      >
        {{ folderPill(current.docroot) }}
      </span>
      <svg
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        class="h-4 w-4 shrink-0 text-neutral-400 transition-transform"
        :class="open ? 'rotate-180' : ''"
      >
        <path stroke-linecap="round" stroke-linejoin="round" d="m6 9 6 6 6-6" />
      </svg>
    </button>

    <FrameworkPickerMenu
      v-if="open"
      :anchor="trigger"
      :selected-key="selectedKey"
      :detected="detected"
      @select="onSelect"
      @close="onMenuClose"
    />

    <div v-if="selectedKey === 'custom'" class="mt-2">
      <div
        class="glass-inset flex items-center gap-2 rounded-lg px-3 focus-within:border-accent-400/70"
      >
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          class="h-4 w-4 shrink-0 text-neutral-400"
        >
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"
          />
        </svg>
        <input
          ref="customInput"
          v-model="customFolder"
          type="text"
          placeholder="public"
          aria-label="Folder to serve, inside the project"
          :disabled="disabled"
          class="w-full bg-transparent py-2 font-mono text-sm text-neutral-900 outline-none placeholder:text-neutral-400 dark:text-neutral-100"
          @change="emitCustom"
          @keydown.enter.prevent="emitCustom"
        />
      </div>
      <p class="mt-1 text-xs text-neutral-500">
        A folder inside the project, like <span class="font-mono">public</span> or
        <span class="font-mono">src/web</span>. Press Enter to apply.
      </p>
    </div>
  </div>
</template>
