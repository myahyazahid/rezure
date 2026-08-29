import { computed, onMounted, onUnmounted, ref } from 'vue'

export function useUptime() {
  const startedAt = Date.now()
  const elapsedMs = ref(0)
  let timer: ReturnType<typeof setInterval> | undefined

  onMounted(() => {
    timer = setInterval(() => {
      elapsedMs.value = Date.now() - startedAt
    }, 1000)
  })

  onUnmounted(() => {
    if (timer) clearInterval(timer)
  })

  const label = computed(() => {
    const totalMinutes = Math.floor(elapsedMs.value / 60000)
    const hours = Math.floor(totalMinutes / 60)
    const minutes = totalMinutes % 60
    return `${hours}h ${minutes}m`
  })

  return { label }
}
