<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { BinaryStatus } from '@/types/binary'

/**
 * The step before installing one of Microsoft's packages: what's about to
 * happen to this machine, the license, and a box only the user can tick.
 *
 * Rezure never accepts a license on anyone's behalf — the installer refuses
 * to run silently without that acceptance, and this box is where it comes
 * from. See `useLicensedInstall`.
 */
const props = defineProps<{ pkg: BinaryStatus }>()
const emit = defineEmits<{ confirm: []; close: [] }>()

const accepted = ref(false)
const linkError = ref<string | null>(null)

async function openLicense() {
  if (!props.pkg.licenseUrl) return
  linkError.value = null
  try {
    await invoke('open_external_link', { url: props.pkg.licenseUrl })
  } catch (e) {
    linkError.value = typeof e === 'string' ? e : 'The license page could not be opened.'
  }
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') emit('close')
}

onMounted(() => window.addEventListener('keydown', onKeydown))
onUnmounted(() => window.removeEventListener('keydown', onKeydown))
</script>

<template>
  <div
    class="glass-scrim fixed inset-0 z-50 flex items-center justify-center p-4"
    @click.self="emit('close')"
  >
    <div class="glass-strong w-full max-w-md rounded-3xl p-6">
      <h2 class="text-xl font-bold tracking-tight">Install {{ pkg.name }}</h2>
      <p class="mt-1 text-sm text-neutral-500">Version {{ pkg.version }}, from Microsoft.</p>

      <ul class="mt-4 flex flex-col gap-2 text-sm text-neutral-600 dark:text-neutral-300">
        <li>
          Rezure downloads Microsoft's installer, checks its checksum and signature, then runs it.
        </li>
        <li>
          Windows asks for administrator permission <strong class="font-semibold">once</strong> —
          click Yes on that prompt.
        </li>
        <li>
          It becomes a normal Windows install, listed in Settings → Apps. Uninstalling Rezure
          doesn't remove it.
        </li>
      </ul>

      <button
        type="button"
        class="mt-4 text-sm font-semibold text-accent-600 underline dark:text-accent-400"
        @click="openLicense"
      >
        Read Microsoft's license terms
      </button>
      <p v-if="linkError" class="mt-1 text-xs text-red-600 dark:text-red-400">{{ linkError }}</p>

      <label class="mt-4 flex items-start gap-2.5">
        <input v-model="accepted" type="checkbox" class="mt-0.5 accent-accent-600" />
        <span class="text-sm text-neutral-700 dark:text-neutral-200">
          I accept the Microsoft license terms for {{ pkg.name }}.
        </span>
      </label>

      <div class="mt-5 flex justify-end gap-2">
        <button
          type="button"
          class="glass-btn rounded-full px-4 py-2 text-sm font-semibold text-neutral-700 transition dark:text-neutral-200"
          @click="emit('close')"
        >
          Cancel
        </button>
        <button
          type="button"
          class="glass-accent rounded-full px-4 py-2 text-sm font-semibold transition disabled:cursor-not-allowed disabled:opacity-50"
          :disabled="!accepted"
          :title="accepted ? '' : 'Accept the license first'"
          @click="emit('confirm')"
        >
          Install
        </button>
      </div>
    </div>
  </div>
</template>
