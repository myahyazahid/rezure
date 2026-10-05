<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue'
import QRCode from 'qrcode'
import type { CryptoWallet } from '@/types/donate'

/**
 * One wallet's QR code, large enough to scan from across a desk.
 *
 * The small one in the wallet's card is a thumbnail; a phone camera needs a
 * bigger target than 64px. Drawn here from the address, on this machine — never
 * by a QR service, which would be handed the wallet address — onto white
 * whatever the theme, because a QR is read as dark modules on a light field.
 */
const props = defineProps<{ wallet: CryptoWallet; copied: boolean }>()
const emit = defineEmits<{ close: []; copy: [] }>()

const qrUrl = ref('')
const closeButton = ref<HTMLButtonElement | null>(null)

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') emit('close')
}

onMounted(async () => {
  window.addEventListener('keydown', onKeydown)
  closeButton.value?.focus()
  // Twice the displayed size, so it stays sharp on a high-density screen; the
  // quiet zone (margin) is what lets a scanner find the edges.
  qrUrl.value = await QRCode.toDataURL(props.wallet.address, {
    margin: 2,
    width: 640,
    errorCorrectionLevel: 'M',
  })
})
onUnmounted(() => window.removeEventListener('keydown', onKeydown))
</script>

<template>
  <div
    class="glass-scrim fixed inset-0 z-50 flex items-center justify-center p-4"
    @click.self="emit('close')"
  >
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="wallet-qr-title"
      class="glass-strong max-h-[92vh] w-full max-w-sm overflow-y-auto rounded-2xl p-6 text-center"
    >
      <div class="flex items-center justify-center gap-2">
        <img
          v-if="wallet.icon"
          :src="wallet.icon.dataUrl"
          alt=""
          draggable="false"
          class="h-6 w-auto max-w-14 shrink-0 object-contain"
        />
        <h2 id="wallet-qr-title" class="text-base font-bold text-neutral-900 dark:text-neutral-100">
          {{ wallet.label }} ({{ wallet.symbol }})
        </h2>
      </div>
      <span
        v-if="wallet.network"
        class="mt-2 inline-block rounded-full bg-accent-500/15 px-2.5 py-0.5 text-xs font-semibold text-accent-700 dark:text-accent-300"
      >
        {{ wallet.network }}
      </span>

      <div
        class="mx-auto mt-4 flex h-72 w-72 max-w-full items-center justify-center rounded-xl bg-white p-2"
      >
        <img
          v-if="qrUrl"
          :src="qrUrl"
          :alt="`${wallet.symbol}${wallet.network ? ` on ${wallet.network}` : ''} address QR code`"
          draggable="false"
          class="h-full w-full object-contain"
        />
        <span v-else class="text-xs text-neutral-500">Making the QR code…</span>
      </div>

      <p
        v-if="wallet.network"
        class="mt-3 text-xs font-medium text-neutral-600 dark:text-neutral-300"
      >
        Send only on the {{ wallet.network }} network — funds sent on another usually can't be
        recovered.
      </p>

      <code
        class="mt-3 block font-mono text-sm break-all text-balance text-neutral-800 dark:text-neutral-200"
      >
        {{ wallet.address }}
      </code>

      <div class="mt-5 flex justify-center gap-2">
        <button
          type="button"
          class="glass-btn rounded-full px-4 py-2 text-sm font-semibold text-neutral-700 transition dark:text-neutral-200"
          @click="emit('copy')"
        >
          {{ copied ? 'Copied!' : 'Copy address' }}
        </button>
        <button
          ref="closeButton"
          type="button"
          class="glass-accent rounded-full px-4 py-2 text-sm font-semibold transition"
          @click="emit('close')"
        >
          Close
        </button>
      </div>
    </div>
  </div>
</template>
