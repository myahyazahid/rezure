<script setup lang="ts">
import { onActivated, ref, watch } from 'vue'
import { RouterLink } from 'vue-router'
import QRCode from 'qrcode'
import { useDonateStore } from '@/stores/donate'

const store = useDonateStore()
const qrDataUrls = ref<Record<string, string>>({})

onActivated(() => {
  store.fetchConfig()
})

// Generated client-side from the address the API returned — never a
// third-party QR service, which would otherwise leak wallet addresses to
// whoever runs it.
watch(
  () => store.config?.crypto,
  async (wallets) => {
    if (!wallets || wallets.length === 0) return
    const entries = await Promise.all(
      wallets.map(
        async (w) =>
          [w.symbol, await QRCode.toDataURL(w.address, { margin: 1, width: 160 })] as const,
      ),
    )
    qrDataUrls.value = Object.fromEntries(entries)
  },
  { immediate: true },
)
</script>

<template>
  <section>
    <h1 class="text-2xl font-semibold text-neutral-900 dark:text-neutral-100">Support Developer</h1>

    <p v-if="store.loading && !store.config" class="mt-6 text-sm text-neutral-500">Loading…</p>

    <template v-else-if="store.config">
      <p class="mt-1 text-sm text-neutral-500">
        {{ store.config.message }}
        <RouterLink
          to="/about"
          class="font-semibold text-red-600 hover:underline dark:text-red-400"
        >
          Read the story
        </RouterLink>
      </p>

      <div v-if="store.config.local.length > 0" class="glass mt-6 rounded-2xl p-4">
        <h2 class="text-sm font-semibold text-neutral-900 dark:text-neutral-100">
          Local (Indonesia)
        </h2>
        <div class="mt-3 flex flex-wrap gap-2">
          <button
            v-for="link in store.config.local"
            :key="link.label"
            type="button"
            class="glass-accent rounded-full px-4 py-2 text-sm font-semibold transition"
            @click="store.openLink(link.url)"
          >
            {{ link.label }}
          </button>
        </div>
      </div>

      <div v-if="store.config.global.length > 0" class="glass mt-4 rounded-2xl p-4">
        <h2 class="text-sm font-semibold text-neutral-900 dark:text-neutral-100">Global</h2>
        <div class="mt-3 flex flex-wrap gap-2">
          <button
            v-for="link in store.config.global"
            :key="link.label"
            type="button"
            class="glass-btn rounded-full px-4 py-2 text-sm font-semibold text-neutral-700 transition dark:text-neutral-200"
            @click="store.openLink(link.url)"
          >
            {{ link.label }}
          </button>
        </div>
      </div>

      <div v-if="store.config.crypto.length > 0" class="glass mt-4 rounded-2xl p-4">
        <h2 class="text-sm font-semibold text-neutral-900 dark:text-neutral-100">Crypto</h2>
        <div class="mt-3 space-y-3">
          <div
            v-for="wallet in store.config.crypto"
            :key="wallet.symbol"
            class="glass-inset flex flex-wrap items-center gap-3 rounded-xl p-3"
          >
            <img
              v-if="qrDataUrls[wallet.symbol]"
              :src="qrDataUrls[wallet.symbol]"
              :alt="`${wallet.symbol} address QR code`"
              class="h-16 w-16 shrink-0 rounded-lg bg-white p-1"
            />
            <div class="min-w-0 flex-1">
              <p class="text-xs font-semibold text-neutral-500">
                {{ wallet.label }} ({{ wallet.symbol }})
              </p>
              <code class="block truncate font-mono text-sm text-neutral-800 dark:text-neutral-200">
                {{ wallet.address }}
              </code>
            </div>
            <button
              type="button"
              class="glass-btn shrink-0 rounded-full px-3 py-1 text-xs font-semibold text-neutral-700 transition dark:text-neutral-200"
              @click="store.copyAddress(wallet.symbol, wallet.address)"
            >
              {{ store.copiedAddress === wallet.symbol ? 'Copied!' : 'Copy' }}
            </button>
          </div>
        </div>
      </div>

      <p
        v-if="
          store.config.local.length === 0 &&
          store.config.global.length === 0 &&
          store.config.crypto.length === 0
        "
        class="glass mt-6 rounded-2xl p-5 text-sm text-neutral-500"
      >
        No donation options are configured yet.
      </p>
    </template>
  </section>
</template>
