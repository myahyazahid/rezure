<script setup lang="ts">
/**
 * A floating status bar for slow backend work.
 *
 * Starting the services or switching PHP spawns real processes and takes
 * seconds, and without any sign of life the window looks frozen.
 *
 * Deliberately *not* a full-screen scrim: the interesting feedback during a
 * bulk start is the service rows flipping to running one by one, and dimming
 * the page hides exactly what the user wants to watch. So this sits above the
 * content without covering it, and never swallows clicks — the callers stop a
 * double-submit by disabling their own control, which is where that belongs.
 *
 * Rendered in a `<Teleport>` so it escapes whatever card or scroll container
 * the caller happens to sit in, at `z-60` so it clears the modals — which sit
 * at `z-50` — rather than relying on which one happens to render last.
 */
import LeafLoader from '@/components/common/LeafLoader.vue'

withDefaults(
  defineProps<{
    show: boolean
    /** What is happening, in the user's words — "Starting services…" */
    label?: string
    /** Optional second line, e.g. which version is being applied. */
    detail?: string
  }>(),
  { label: 'Working…', detail: '' },
)
</script>

<template>
  <Teleport to="body">
    <Transition name="busy-rise">
      <div
        v-if="show"
        class="pointer-events-none fixed inset-0 z-60 flex items-center justify-center px-6"
        role="status"
        aria-live="polite"
      >
        <div
          class="flex flex-col items-center gap-3 rounded-2xl border border-neutral-200 bg-white/95 px-7 py-5 shadow-2xl shadow-neutral-900/15 backdrop-blur-sm dark:border-neutral-700 dark:bg-neutral-900/95 dark:shadow-black/50"
        >
          <LeafLoader :size="56" />
          <div class="text-center">
            <p class="text-sm font-semibold text-neutral-800 dark:text-neutral-100">{{ label }}</p>
            <p v-if="detail" class="mt-0.5 text-xs text-neutral-500 dark:text-neutral-400">
              {{ detail }}
            </p>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.busy-rise-enter-active,
.busy-rise-leave-active {
  transition:
    opacity 180ms ease,
    transform 180ms cubic-bezier(0.4, 0, 0.2, 1);
}

/* Scales rather than rising: the card sits in the middle of the window, and
   sliding a centred element in from an edge reads as a misplaced toast. */
.busy-rise-enter-from,
.busy-rise-leave-to {
  opacity: 0;
  transform: scale(0.94);
}

@media (prefers-reduced-motion: reduce) {
  .busy-rise-enter-active,
  .busy-rise-leave-active {
    transition: opacity 180ms ease;
  }

  .busy-rise-enter-from,
  .busy-rise-leave-to {
    transform: none;
  }
}
</style>
