import { defineStore } from 'pinia'
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { ProjectInfo } from '@/types/project'

export const useProjectsStore = defineStore('projects', () => {
  const projects = ref<ProjectInfo[]>([])

  async function fetchAll() {
    projects.value = await invoke<ProjectInfo[]>('list_projects')
  }

  return { projects, fetchAll }
})
