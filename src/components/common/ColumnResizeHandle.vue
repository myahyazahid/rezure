<script setup lang="ts">
/** The drag handle on a resizable column's heading (see
 *  composables/useResizableColumns). Place it inside the heading, which
 *  must be `relative` and not clip it; bind `pointerdown` and `dblclick`
 *  on it, they fall through to the handle. The row of headings carries
 *  `group/cols`, so the dividers show while the pointer is over it. */
defineProps<{ active: boolean }>()
</script>

<template>
  <!-- Covers the gap to the next heading and the header's full height
       (`py-3`), so it's easy to grab. The divider stays hidden at rest: drawn
       all the time it turned the glass card into a spreadsheet grid. -->
  <span
    class="group/handle absolute -top-3 -right-3 -bottom-3 z-10 flex w-3 cursor-col-resize justify-center py-2.5"
    title="Drag to resize, double-click to fit the window"
  >
    <span
      class="w-0.5 rounded-full transition"
      :class="
        active
          ? 'bg-accent-500'
          : 'bg-neutral-300 opacity-0 group-hover/cols:opacity-100 group-hover/handle:bg-accent-400 dark:bg-white/20'
      "
    ></span>
  </span>
</template>
