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
import { computed } from 'vue'
import LeafLoader from '@/components/common/LeafLoader.vue'

const props = withDefaults(
  defineProps<{
    show: boolean
    /** What is happening, in the user's words — "Starting services…" */
    label?: string
    /** Optional second line, e.g. which version is being applied. */
    detail?: string
    /** 0-100, when the caller has a real number to show — an export
     *  tracking bytes written, say. `null`/omitted keeps the plain spinner,
     *  which is still the right call for anything that can't estimate how
     *  much of it is left. */
    percent?: number | null
    /** Shown next to the bar when `percent` is set — stops a double-submit
     *  the same way every other button here does, by disabling itself. */
    onCancel?: () => void
  }>(),
  { label: 'Working…', detail: '', percent: null, onCancel: undefined },
)

const showBar = computed(() => props.percent !== null)
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
          class="pointer-events-auto flex flex-col items-center gap-3 rounded-2xl border border-neutral-200 bg-white/95 px-7 py-5 shadow-2xl shadow-neutral-900/15 backdrop-blur-sm dark:border-neutral-700 dark:bg-neutral-900/95 dark:shadow-black/50"
        >
          <LeafLoader :size="56" />
          <div class="text-center">
            <p class="text-sm font-semibold text-neutral-800 dark:text-neutral-100">{{ label }}</p>
            <p v-if="detail" class="mt-0.5 text-xs text-neutral-500 dark:text-neutral-400">
              {{ detail }}
            </p>
          </div>

          <!-- Only for a caller with a real number, like an export tracking
               bytes written — everything else keeps the plain spinner
               above, which is honest about not knowing how much is left. -->
          <div v-if="showBar" class="w-56">
            <div
              class="h-1.5 w-full overflow-hidden rounded-full bg-neutral-200 dark:bg-neutral-700"
            >
              <div
                class="h-full rounded-full bg-red-500 transition-[width] duration-300 ease-out"
                :style="{ width: `${percent}%` }"
              />
            </div>
            <p class="mt-1.5 text-center text-xs text-neutral-500 dark:text-neutral-400">
              {{ percent }}%
            </p>
          </div>

          <button
            v-if="onCancel"
            type="button"
            class="mt-1 rounded-full border border-neutral-200 px-4 py-1.5 text-xs font-semibold text-neutral-600 transition hover:border-neutral-300 hover:text-neutral-900 dark:border-neutral-700 dark:text-neutral-300 dark:hover:text-neutral-50"
            @click="onCancel"
          >
            Cancel
          </button>
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
