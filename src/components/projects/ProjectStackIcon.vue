<script setup lang="ts">
import { computed } from 'vue'
import { useProjectsStore } from '@/stores/projects'
import { stackTooltip } from '@/composables/projectStack'
import type { ProjectStack } from '@/types/project'

/**
 * The "i" beside a project's name: what the project is built on and needs.
 * Amber when something doesn't fit, quiet otherwise. Hover for the short
 * version; click for the dialog with every requirement and its fix.
 */
const props = defineProps<{
  projectId: string
  stack: ProjectStack | null
}>()

const store = useProjectsStore()

const tooltip = computed(() => (props.stack ? stackTooltip(props.stack) : ''))
</script>

<template>
  <button
    v-if="stack"
    type="button"
    class="flex h-5 w-5 shrink-0 items-center justify-center rounded-full transition"
    :class="
      stack.needsAttention
        ? 'bg-amber-500/15 text-amber-600 hover:bg-amber-500/25 dark:text-amber-400'
        : 'text-neutral-400 hover:bg-neutral-500/10 hover:text-neutral-600 dark:hover:text-neutral-200'
    "
    :title="tooltip"
    :aria-label="tooltip"
    @click="store.openStackModal(projectId)"
  >
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2.25"
      class="h-3.5 w-3.5"
    >
      <circle v-if="!stack.needsAttention" cx="12" cy="12" r="9" />
      <path stroke-linecap="round" d="M12 11v6M12 7.5v.01" />
    </svg>
  </button>
</template>
