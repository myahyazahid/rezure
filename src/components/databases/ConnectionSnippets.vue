<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import type { ConnectionSnippet } from '@/types/database'
import { usePostgresUsersStore } from '@/stores/postgresUsers'

/**
 * The settings that point an app at one PostgreSQL user, in the forms the
 * common frameworks read — written in Rust (`postgres_roles::snippets`),
 * only shown and copied here.
 */
const props = defineProps<{
  user: string
  /** What was just typed for it, if anything — Rezure doesn't keep it. */
  password: string | null
  database: string | null
}>()

const store = usePostgresUsersStore()

const snippets = ref<ConnectionSnippet[]>([])
const selected = ref('laravel')
const copied = ref(false)

const current = computed(
  () => snippets.value.find((s) => s.id === selected.value) ?? snippets.value[0] ?? null,
)

watch(
  () => [props.user, props.password, props.database],
  async () => {
    snippets.value = await store.snippets(props.user, props.password, props.database)
  },
  { immediate: true },
)

async function copy() {
  if (!current.value) return
  await navigator.clipboard.writeText(current.value.text)
  copied.value = true
  setTimeout(() => (copied.value = false), 1500)
}
</script>

<template>
  <div>
    <div class="flex flex-wrap gap-1">
      <button
        v-for="snippet in snippets"
        :key="snippet.id"
        type="button"
        class="rounded-full px-2.5 py-1 text-[11px] font-semibold transition"
        :class="
          current?.id === snippet.id
            ? 'glass-raised text-neutral-900 dark:text-neutral-50'
            : 'text-neutral-500 hover:text-neutral-800 dark:hover:text-neutral-200'
        "
        @click="selected = snippet.id"
      >
        {{ snippet.label }}
      </button>
    </div>
    <div v-if="current" class="relative mt-2">
      <pre
        class="max-h-48 overflow-auto rounded-xl bg-neutral-900/90 p-3 pr-16 font-mono text-[11px] leading-relaxed whitespace-pre-wrap text-neutral-100 dark:bg-black/40"
        >{{ current.text }}</pre>
      <button
        type="button"
        class="absolute top-2 right-2 rounded-md px-2 py-1 text-[11px] font-semibold text-neutral-200 transition hover:bg-white/10"
        @click="copy"
      >
        {{ copied ? 'Copied' : 'Copy' }}
      </button>
    </div>
    <p v-if="!password" class="mt-1.5 text-[11px] text-neutral-500">
      No password filled in. Rezure's PostgreSQL lets local connections in without one, but put the
      real one here before using these settings anywhere else.
    </p>
  </div>
</template>
