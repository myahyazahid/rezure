<script setup lang="ts">
import { computed } from 'vue'

const props = defineProps<{ values: number[] }>()

const WIDTH = 120
const HEIGHT = 26

/** Maps samples (0-100) onto the viewBox, leaving 1px of headroom for the stroke. */
const points = computed(() => {
  const values = props.values
  if (values.length < 2) return ''

  const step = WIDTH / (values.length - 1)
  return values
    .map((value, i) => {
      const clamped = Math.min(100, Math.max(0, value))
      const y = HEIGHT - 1 - (clamped / 100) * (HEIGHT - 2)
      return `${(i * step).toFixed(1)},${y.toFixed(1)}`
    })
    .join(' ')
})

const areaPoints = computed(() =>
  points.value ? `0,${HEIGHT} ${points.value} ${WIDTH},${HEIGHT}` : '',
)
</script>

<template>
  <svg
    v-if="points"
    :viewBox="`0 0 ${WIDTH} ${HEIGHT}`"
    :width="WIDTH"
    :height="HEIGHT"
    preserveAspectRatio="none"
    aria-hidden="true"
    class="text-emerald-500"
  >
    <polygon :points="areaPoints" fill="currentColor" fill-opacity="0.15" />
    <polyline
      :points="points"
      fill="none"
      stroke="currentColor"
      stroke-width="1.5"
      stroke-linecap="round"
      stroke-linejoin="round"
      vector-effect="non-scaling-stroke"
    />
  </svg>
</template>
