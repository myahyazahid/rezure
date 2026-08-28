import { defineStore } from 'pinia'
import { ref } from 'vue'
import type { ServiceInfo } from '@/types/service'

export const useServicesStore = defineStore('services', () => {
  const services = ref<ServiceInfo[]>([])

  return { services }
})
