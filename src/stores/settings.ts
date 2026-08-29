import { defineStore } from 'pinia'
import { ref } from 'vue'

export const useSettingsStore = defineStore('settings', () => {
  const defaultPort = ref(80)
  const shareUsageData = ref(false)

  return { defaultPort, shareUsageData }
})
