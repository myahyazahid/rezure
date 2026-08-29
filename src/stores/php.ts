import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { PhpVersion } from '@/types/php'

export const usePhpStore = defineStore('php', () => {
  const versions = ref<PhpVersion[]>([])

  const active = computed(() => versions.value.find((v) => v.active) ?? null)

  async function fetchAll() {
    versions.value = await invoke<PhpVersion[]>('list_php_versions')
  }

  async function setActive(id: string) {
    versions.value = await invoke<PhpVersion[]>('set_active_php_version', { id })
  }

  return { versions, active, fetchAll, setActive }
})
