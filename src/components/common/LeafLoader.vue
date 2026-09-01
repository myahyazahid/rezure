<script setup lang="ts">
/**
 * The Rezure mark as a loading animation.
 *
 * Eight leaves cycle through three formations — a spinning ring, a scatter,
 * and the letter R — and back again. Every formation uses the *same* eight
 * leaves, so nothing morphs: only each leaf's position, rotation and scale
 * change, which the browser interpolates as a plain CSS transform transition.
 * That keeps it on the compositor and costs nothing next to path morphing.
 *
 * The R layout was tuned against the real logo by rendering it to an image
 * and checking it by eye, including at 48px where a loader actually lives.
 */
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'

const props = withDefaults(
  defineProps<{
    /** Rendered width and height in px. */
    size?: number
    /** Announced to screen readers, and shown under the mark when set. */
    label?: string
  }>(),
  { size: 48, label: '' },
)

/** One leaf, centred on the origin and pointing up. Two mirrored cubics
 *  meeting at sharp tips — the lens shape the Rezure logo is built from. */
const LEAF = 'M0,-18 C5.5,-10 5.5,8 0,18 C-5.5,8 -5.5,-10 0,-18 Z'

const COUNT = 8
const CENTER = 50

type Placement = { x: number; y: number; rot: number; scale: number }
type Formation = 'ring' | 'scatter' | 'letter'

/** Leaves evenly spaced on a circle, each canted off its radius so the ring
 *  reads as a pinwheel rather than a static flower. `spin` rotates the whole
 *  arrangement — stepping it in small increments is what makes it turn. */
function ring(spin: number): Placement[] {
  return Array.from({ length: COUNT }, (_, i) => {
    const angle = i * (360 / COUNT) + spin
    const rad = (angle * Math.PI) / 180
    return {
      x: CENTER + 27 * Math.sin(rad),
      y: CENTER - 27 * Math.cos(rad),
      rot: angle + 38,
      scale: 0.62,
    }
  })
}

/** Flung outward and spun off-axis — the beat between the two readable
 *  shapes. The offsets are deliberately irregular so it looks thrown rather
 *  than mechanically exploded. */
const SCATTER_SPIN = [20, -35, 55, -70, 95, -15, 140, -110]
function scatter(): Placement[] {
  return Array.from({ length: COUNT }, (_, i) => {
    const angle = i * (360 / COUNT) + 22
    const rad = (angle * Math.PI) / 180
    return {
      x: CENTER + 46 * Math.sin(rad),
      y: CENTER - 46 * Math.cos(rad),
      rot: angle + (SCATTER_SPIN[i] ?? 0),
      scale: 0.45,
    }
  })
}

/** The letter, from the same eight leaves. */
const LETTER: Placement[] = [
  { x: 31, y: 33, rot: 5, scale: 1.22 }, // left stem, upper
  { x: 31, y: 66, rot: -4, scale: 1.18 }, // left stem, lower
  { x: 54, y: 17, rot: 97, scale: 1.15 }, // bowl, top sweep
  { x: 66, y: 33, rot: 150, scale: 1.0 }, // bowl, right descender
  { x: 61, y: 42, rot: 182, scale: 0.65 }, // bowl, closing curl
  { x: 49, y: 53, rot: 84, scale: 1.05 }, // middle bar
  { x: 57, y: 63, rot: 131, scale: 0.85 }, // leg, upper
  { x: 68, y: 77, rot: 137, scale: 1.0 }, // leg, lower
]

type Frame = { formation: Formation; spinBy?: number; transition: number; hold: number }

// Eight 45-degree steps make one full turn. Stepping by a whole 90 or 180
// would cut visible chords across the circle, because a transform transition
// moves each leaf in a straight line rather than along the arc.
const SPIN_STEPS: Frame[] = Array.from({ length: COUNT }, () => ({
  formation: 'ring' as const,
  spinBy: 360 / COUNT,
  transition: 150,
  hold: 0,
}))

const TIMELINE: Frame[] = [
  ...SPIN_STEPS,
  { formation: 'scatter', transition: 380, hold: 140 },
  { formation: 'letter', transition: 520, hold: 900 },
  { formation: 'scatter', transition: 380, hold: 140 },
]

const formation = ref<Formation>('ring')
const spin = ref(0)
const duration = ref(150)
const step = ref(0)

const reduceMotion = ref(false)
let timer: ReturnType<typeof setTimeout> | undefined

const placements = computed<Placement[]>(() => {
  if (formation.value === 'letter') return LETTER
  if (formation.value === 'scatter') return scatter()
  return ring(spin.value)
})

function leafStyle(i: number) {
  const p = placements.value[i]
  if (!p) return {}
  return {
    transform: `translate(${p.x}px, ${p.y}px) rotate(${p.rot}deg) scale(${p.scale})`,
    transitionDuration: `${duration.value}ms`,
  }
}

function advance() {
  const frame = TIMELINE[step.value % TIMELINE.length]
  // The modulo keeps this in range; the guard is here because the compiler
  // can't prove that, and silently stopping beats a crash inside a timer.
  if (!frame) return
  formation.value = frame.formation
  duration.value = frame.transition
  if (frame.spinBy) spin.value += frame.spinBy
  step.value += 1
  timer = setTimeout(advance, frame.transition + frame.hold)
}

onMounted(() => {
  // Someone who has asked the OS for less motion gets the ring, still, rather
  // than a mark that flies apart four times a second.
  const query = window.matchMedia('(prefers-reduced-motion: reduce)')
  reduceMotion.value = query.matches
  if (!reduceMotion.value) advance()
})

onBeforeUnmount(() => {
  if (timer) clearTimeout(timer)
})
</script>

<template>
  <!-- Only announces itself when it carries its own label. Inside BusyOverlay
       the wrapper is already the live region, and a nested one makes a screen
       reader read the same thing twice. -->
  <div
    class="flex flex-col items-center gap-2"
    :role="props.label ? 'status' : undefined"
    :aria-hidden="props.label ? undefined : 'true'"
  >
    <svg
      :width="props.size"
      :height="props.size"
      viewBox="0 0 100 100"
      aria-hidden="true"
      class="text-red-600 dark:text-red-500"
      :class="{ 'animate-pulse': reduceMotion }"
    >
      <path
        v-for="i in COUNT"
        :key="i"
        :d="LEAF"
        fill="currentColor"
        class="leaf"
        :style="leafStyle(i - 1)"
      />
    </svg>

    <p v-if="props.label" class="text-xs font-medium text-neutral-500 dark:text-neutral-400">
      {{ props.label }}
    </p>
  </div>
</template>

<style scoped>
.leaf {
  /* Eased rather than linear: the leaves settle into each formation instead of
     arriving at full speed. The spin steps are short enough that the easing
     reads as momentum, not as eight separate moves. */
  transition-property: transform;
  transition-timing-function: cubic-bezier(0.4, 0, 0.2, 1);
}

@media (prefers-reduced-motion: reduce) {
  .leaf {
    transition: none;
  }
}
</style>
