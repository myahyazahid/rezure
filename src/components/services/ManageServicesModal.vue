<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import type { ManagedService } from '@/types/service'
import { useServicesStore } from '@/stores/services'
import BasePill from '@/components/common/BasePill.vue'
import LeafLoader from '@/components/common/LeafLoader.vue'
import TechIcon from '@/components/common/TechIcon.vue'

/**
 * Picks which services the Services page shows. Someone on MySQL has no use
 * for a SQL Server card, and the reverse. Removing one takes it off the page
 * and out of Start all — it isn't uninstalled, and its data stays. What the
 * list holds, and stopping a running service before it's removed, is decided
 * in Rust (`services::service_visibility`).
 */
const emit = defineEmits<{ close: [] }>()

const store = useServicesStore()

const loading = ref(true)
const error = ref<string | null>(null)
/** The row whose Add or Remove is in flight. One at a time: removing a
 *  running database waits on its clean shutdown. */
const busyId = ref<string | null>(null)

const added = computed(() => store.managed.filter((s) => s.shown))
const notAdded = computed(() => store.managed.filter((s) => !s.shown))

function errorMessage(e: unknown): string {
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  return 'Something went wrong.'
}

async function load() {
  try {
    await store.fetchManaged()
  } catch (e) {
    error.value = errorMessage(e)
  } finally {
    loading.value = false
  }
}

async function setShown(service: ManagedService, shown: boolean) {
  if (busyId.value) return
  busyId.value = service.id
  error.value = null
  try {
    await store.setShown(service.id, shown)
  } catch (e) {
    error.value = errorMessage(e)
  } finally {
    busyId.value = null
  }
}

/** What adding or removing this row means, beyond the obvious. */
function notes(service: ManagedService): string[] {
  const list: string[] = []
  if (service.shown && service.running) list.push('Running — removing it stops it first')
  if (service.shown && service.servesSites) list.push('Your project sites need it')
  if (!service.installed) {
    list.push(service.shown ? 'Not installed' : 'Not installed — its card offers Install')
  }
  return list
}

function busyLabel(service: ManagedService): string {
  if (!service.shown) return 'Adding…'
  return service.running ? 'Stopping…' : 'Removing…'
}

function close() {
  if (busyId.value) return
  emit('close')
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') close()
}

onMounted(() => {
  window.addEventListener('keydown', onKeydown)
  load()
})
onUnmounted(() => window.removeEventListener('keydown', onKeydown))

const ROW_BUTTON_CLASS =
  'shrink-0 rounded-full px-3.5 py-1.5 text-xs font-semibold transition disabled:cursor-not-allowed disabled:opacity-50'
</script>

<template>
  <div
    class="glass-scrim fixed inset-0 z-50 flex items-center justify-center p-4"
    @click.self="close"
  >
    <div class="glass-strong flex max-h-[85vh] w-full max-w-lg flex-col rounded-3xl p-6">
      <h2 class="text-xl font-bold tracking-tight">Manage services</h2>
      <p class="mt-1 text-sm text-neutral-500">
        Choose what the Services page shows. Removing a service doesn't uninstall it or touch its
        data — add it back any time.
      </p>

      <div v-if="loading" class="my-10 flex justify-center">
        <LeafLoader :size="40" label="Loading services…" />
      </div>

      <div v-else class="-mx-1 mt-4 flex-1 overflow-y-auto px-1">
        <template
          v-for="group in [
            { title: 'On the Services page', items: added, shown: true },
            { title: 'Not added', items: notAdded, shown: false },
          ]"
          :key="group.title"
        >
          <section v-if="group.shown || group.items.length > 0" class="mb-4 last:mb-0">
            <h3
              class="mb-2 text-xs font-semibold tracking-wide text-neutral-500 uppercase dark:text-neutral-400"
            >
              {{ group.title }}
            </h3>
            <p v-if="group.items.length === 0" class="text-sm text-neutral-500">
              Nothing yet — add a service below.
            </p>
            <ul class="flex flex-col gap-2">
              <li
                v-for="service in group.items"
                :key="service.id"
                class="glass-inset flex items-center gap-3 rounded-2xl px-3 py-2.5"
              >
                <div
                  class="glass flex h-8 w-8 shrink-0 items-center justify-center rounded-full"
                  :class="service.shown ? '' : 'opacity-60 grayscale'"
                >
                  <TechIcon :id="service.id" :size="18" />
                </div>
                <div class="min-w-0 flex-1">
                  <div class="flex items-center gap-2">
                    <span
                      class="truncate text-sm font-semibold text-neutral-900 dark:text-neutral-100"
                      >{{ service.name }}</span
                    >
                    <BasePill class="shrink-0">{{ service.category }}</BasePill>
                  </div>
                  <p v-if="notes(service).length" class="mt-0.5 text-xs text-neutral-500">
                    {{ notes(service).join(' · ') }}
                  </p>
                </div>
                <button
                  v-if="service.shown"
                  type="button"
                  :class="[ROW_BUTTON_CLASS, 'glass-btn text-neutral-700 dark:text-neutral-200']"
                  :disabled="busyId !== null"
                  @click="setShown(service, false)"
                >
                  {{ busyId === service.id ? busyLabel(service) : 'Remove' }}
                </button>
                <button
                  v-else
                  type="button"
                  :class="[ROW_BUTTON_CLASS, 'glass-accent']"
                  :disabled="busyId !== null"
                  @click="setShown(service, true)"
                >
                  {{ busyId === service.id ? busyLabel(service) : 'Add' }}
                </button>
              </li>
            </ul>
          </section>
        </template>
      </div>

      <p v-if="error" class="mt-3 text-sm text-red-600 dark:text-red-400">{{ error }}</p>

      <div class="mt-5 flex justify-end">
        <button
          type="button"
          class="glass-accent rounded-full px-4 py-2 text-sm font-semibold transition disabled:opacity-50"
          :disabled="busyId !== null"
          @click="close"
        >
          Done
        </button>
      </div>
    </div>
  </div>
</template>
