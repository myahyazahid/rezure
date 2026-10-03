import { defineStore } from 'pinia'
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { DonateConfig } from '@/types/donate'

export const useDonateStore = defineStore('donate', () => {
  const config = ref<DonateConfig | null>(null)
  const loading = ref(false)
  const copiedAddress = ref<string | null>(null)

  async function fetchConfig() {
    loading.value = true
    try {
      config.value = await invoke<DonateConfig>('fetch_donate_config')
    } finally {
      loading.value = false
    }
  }

  async function openLink(url: string) {
    await invoke('open_external_link', { url })
  }

  async function copyAddress(symbol: string, address: string) {
    await navigator.clipboard.writeText(address)
    copiedAddress.value = symbol
    setTimeout(() => {
      if (copiedAddress.value === symbol) copiedAddress.value = null
    }, 1500)
  }

  return { config, loading, copiedAddress, fetchConfig, openLink, copyAddress }
})
