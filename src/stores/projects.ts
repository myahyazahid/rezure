import { defineStore } from 'pinia'
import { ref } from 'vue'
import type { ProjectInfo } from '@/types/project'

export const useProjectsStore = defineStore('projects', () => {
  const projects = ref<ProjectInfo[]>([])

  return { projects }
})
