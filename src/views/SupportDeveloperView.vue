<script setup lang="ts">
import { onActivated, ref, watch } from 'vue'
import { RouterLink } from 'vue-router'
import QRCode from 'qrcode'
import WalletQrModal from '@/components/donate/WalletQrModal.vue'
import { useDonateStore } from '@/stores/donate'
import type { CryptoWallet } from '@/types/donate'

const store = useDonateStore()
const qrDataUrls = ref<Record<string, string>>({})

/** The wallet whose QR code is open full size, if any. */
const qrWallet = ref<CryptoWallet | null>(null)

onActivated(() => {
  store.fetchConfig()
})

/** What tells one wallet from another — its QR code, its "Copied!" state, its
 *  row. Never the symbol: the same coin on two networks (USDT on Tron and on
 *  Ethereum) is two wallets with one symbol, and keying by it would put one
 *  wallet's QR code beside the other's address. */
function walletKey(wallet: CryptoWallet): string {
  return wallet.id !== null
    ? String(wallet.id)
    : `${wallet.symbol}|${wallet.network ?? ''}|${wallet.address}`
}

// Generated client-side from the address the API returned — never a
// third-party QR service, which would otherwise leak wallet addresses to
// whoever runs it.
watch(
  () => store.config?.crypto,
  async (wallets) => {
    if (!wallets || wallets.length === 0) {
      qrDataUrls.value = {}
      return
    }
    const entries = await Promise.all(
      wallets.map(
        async (w) =>
          [walletKey(w), await QRCode.toDataURL(w.address, { margin: 1, width: 160 })] as const,
      ),
    )
    qrDataUrls.value = Object.fromEntries(entries)
  },
  { immediate: true },
)
</script>

<template>
  <section>
    <h1 class="text-2xl font-semibold text-neutral-900 dark:text-neutral-100">Donate</h1>

    <p v-if="store.loading && !store.config" class="mt-6 text-sm text-neutral-500">Loading…</p>

    <template v-else-if="store.config">
      <p class="mt-1 text-sm text-neutral-500">
        {{ store.config.message }}
        <RouterLink
          to="/about"
          class="font-semibold text-accent-600 hover:underline dark:text-accent-400"
        >
          Read the story
        </RouterLink>
      </p>

      <!-- The page renders from what was saved last time when the server can't
           be reached; this says so, so a stale link or QRIS isn't a surprise. -->
      <p
        v-if="store.config.offline"
        role="status"
        class="mt-4 rounded-xl bg-amber-100/50 px-3 py-2 text-sm text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
      >
        Can't reach the server right now — showing what was saved last time.
      </p>

      <div
        v-if="store.config.qris || store.config.qrisUnavailable"
        class="glass mt-6 rounded-2xl p-4"
      >
        <h2 class="text-sm font-semibold text-neutral-900 dark:text-neutral-100">QRIS</h2>
        <p class="mt-1 text-xs text-neutral-500">
          Scan with any e-wallet or mobile banking app that supports QRIS.
        </p>
        <!-- Shown exactly as sent: a QRIS must stay scannable, so it is never
             cropped, recoloured or scaled up past its own size. -->
        <img
          v-if="store.config.qris"
          :src="store.config.qris.dataUrl"
          alt="QRIS code for donations"
          draggable="false"
          class="mx-auto mt-3 block max-h-108 w-auto max-w-full rounded-xl bg-white object-contain p-2"
        />
        <p
          v-if="store.config.qrisStale"
          role="status"
          class="mt-3 rounded-xl bg-amber-100/50 px-3 py-2 text-xs text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
        >
          A newer QRIS exists but couldn't be loaded — this one may be out of date. Reopen this page
          when you're online.
        </p>
        <p
          v-else-if="store.config.qrisUnavailable"
          role="status"
          class="mt-3 rounded-xl bg-amber-100/50 px-3 py-2 text-xs text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
        >
          The QRIS couldn't be loaded. Check your connection and reopen this page.
        </p>
      </div>

      <div v-if="store.config.local.length > 0" class="glass mt-6 rounded-2xl p-4">
        <h2 class="text-sm font-semibold text-neutral-900 dark:text-neutral-100">
          Local (Indonesia)
        </h2>
        <div class="mt-3 flex flex-wrap gap-2">
          <button
            v-for="link in store.config.local"
            :key="link.id ?? link.label"
            type="button"
            class="glass-accent inline-flex items-center gap-2 rounded-full px-4 py-2 text-sm font-semibold transition"
            @click="store.openLink(link.url)"
          >
            <img
              v-if="link.icon"
              :src="link.icon.dataUrl"
              alt=""
              draggable="false"
              class="h-4 w-4 shrink-0 object-contain"
            />
            {{ link.label }}
          </button>
        </div>
      </div>

      <div v-if="store.config.global.length > 0" class="glass mt-4 rounded-2xl p-4">
        <h2 class="text-sm font-semibold text-neutral-900 dark:text-neutral-100">Global</h2>
        <div class="mt-3 flex flex-wrap gap-2">
          <button
            v-for="link in store.config.global"
            :key="link.id ?? link.label"
            type="button"
            class="glass-btn inline-flex items-center gap-2 rounded-full px-4 py-2 text-sm font-semibold text-neutral-700 transition dark:text-neutral-200"
            @click="store.openLink(link.url)"
          >
            <img
              v-if="link.icon"
              :src="link.icon.dataUrl"
              alt=""
              draggable="false"
              class="h-4 w-4 shrink-0 object-contain"
            />
            {{ link.label }}
          </button>
        </div>
      </div>

      <div v-if="store.config.crypto.length > 0" class="glass mt-4 rounded-2xl p-4">
        <h2 class="text-sm font-semibold text-neutral-900 dark:text-neutral-100">Crypto</h2>
        <!-- Two columns only on a window wide enough that each card keeps its
             address to two lines; below that, one full-width row apiece. -->
        <div class="mt-3 grid gap-3 xl:grid-cols-2">
          <div
            v-for="wallet in store.config.crypto"
            :key="walletKey(wallet)"
            class="glass-inset flex min-w-0 items-center gap-3 rounded-xl p-3"
          >
            <!-- The thumbnail is too small to scan from a screen; clicking it
                 (or the label under it) opens the QR code full size. -->
            <button
              v-if="qrDataUrls[walletKey(wallet)]"
              type="button"
              class="group flex shrink-0 flex-col items-center gap-1"
              :aria-label="`View the ${wallet.symbol}${wallet.network ? ` on ${wallet.network}` : ''} QR code full size`"
              @click="qrWallet = wallet"
            >
              <img
                :src="qrDataUrls[walletKey(wallet)]"
                alt=""
                draggable="false"
                class="h-16 w-16 rounded-lg bg-white p-1 transition group-hover:scale-105"
              />
              <span
                class="text-[11px] font-semibold text-accent-600 group-hover:underline dark:text-accent-400"
              >
                View QR
              </span>
            </button>
            <div class="min-w-0 flex-1">
              <div class="flex flex-wrap items-center gap-x-2 gap-y-1">
                <img
                  v-if="wallet.icon"
                  :src="wallet.icon.dataUrl"
                  alt=""
                  draggable="false"
                  class="h-5 w-auto max-w-12 shrink-0 object-contain"
                />
                <p class="text-xs font-semibold text-neutral-500">
                  {{ wallet.label }} ({{ wallet.symbol }})
                </p>
                <!-- Sending on the wrong network usually loses the funds, so
                     the network sits right beside the address it belongs to. -->
                <span
                  v-if="wallet.network"
                  class="rounded-full bg-accent-500/15 px-2 py-0.5 text-[10px] font-semibold text-accent-700 dark:text-accent-300"
                >
                  {{ wallet.network }}
                </span>
              </div>
              <!-- Wraps instead of truncating — an address is checked by its
                   ends, and a cut-off one hides the end — and balances the
                   lines, so it breaks in the middle rather than leaving a
                   short tail on a second line. -->
              <code
                class="mt-1 block font-mono text-sm break-all text-balance text-neutral-800 dark:text-neutral-200"
              >
                {{ wallet.address }}
              </code>
            </div>
            <button
              type="button"
              class="glass-btn shrink-0 rounded-full px-3 py-1 text-xs font-semibold text-neutral-700 transition dark:text-neutral-200"
              @click="store.copyAddress(walletKey(wallet), wallet.address)"
            >
              {{ store.copiedAddress === walletKey(wallet) ? 'Copied!' : 'Copy' }}
            </button>
          </div>
        </div>
      </div>

      <p
        v-if="
          store.config.local.length === 0 &&
          store.config.global.length === 0 &&
          store.config.crypto.length === 0 &&
          !store.config.qris &&
          !store.config.qrisUnavailable
        "
        class="glass mt-6 rounded-2xl p-5 text-sm text-neutral-500"
      >
        {{
          store.config.offline
            ? 'Nothing is saved yet, and the server can’t be reached. Try again when you’re online.'
            : 'No donation options are configured yet.'
        }}
      </p>
    </template>

    <WalletQrModal
      v-if="qrWallet"
      :wallet="qrWallet"
      :copied="store.copiedAddress === walletKey(qrWallet)"
      @copy="store.copyAddress(walletKey(qrWallet), qrWallet.address)"
      @close="qrWallet = null"
    />
  </section>
</template>
