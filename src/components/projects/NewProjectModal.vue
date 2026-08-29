<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useProjectsStore } from '@/stores/projects'
import BasePill from '@/components/common/BasePill.vue'

const emit = defineEmits<{ close: [] }>()

const store = useProjectsStore()

const step = ref<1 | 2>(1)
const selectedTemplateId = ref<string | null>(null)
const name = ref('')

const selectedTemplate = computed(
  () => store.templates.find((t) => t.id === selectedTemplateId.value) ?? null,
)

const NAME_PATTERN = /^[a-z0-9]([a-z0-9-]*[a-z0-9])?$/

const nameError = computed(() => {
  const value = name.value.trim()
  if (!value) return null
  if (value.length > 64) return 'Keep it under 64 characters.'
  if (!NAME_PATTERN.test(value)) {
    return 'Lowercase letters, digits, and hyphens only — no spaces.'
  }
  if (store.projects.some((p) => p.id === value)) {
    return `A project named "${value}" already exists.`
  }
  return null
})

const canContinue = computed(() => selectedTemplateId.value !== null)
const canCreate = computed(() => name.value.trim().length > 0 && !nameError.value)

function goToNaming() {
  if (canContinue.value) step.value = 2
}

function goBack() {
  step.value = 1
  store.createError = null
}

function close() {
  // The Rust side keeps scaffolding even if this dialog closes — but that's
  // confusing to walk away from mid-create, so keep it open and visible
  // until the result (success or error) actually comes back.
  if (store.creating) return
  emit('close')
}

async function submit() {
  if (!canCreate.value || !selectedTemplateId.value) return
  const ok = await store.createProject(name.value.trim(), selectedTemplateId.value)
  if (ok) close()
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') close()
}

onMounted(() => {
  window.addEventListener('keydown', onKeydown)
  if (store.templates.length === 0) store.fetchTemplateInfo()
})
onUnmounted(() => window.removeEventListener('keydown', onKeydown))

// Starting over after a create failure should feel like a slate wipe, not
// a return to a stale error message.
watch(name, () => {
  store.createError = null
})
</script>

<template>
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4 backdrop-blur-sm"
    @click.self="close"
  >
    <div class="w-full max-w-md rounded-3xl bg-white p-6 shadow-2xl dark:bg-neutral-900">
      <div class="flex items-start justify-between gap-4">
        <div>
          <h2 class="text-xl font-bold tracking-tight">New project</h2>
          <p class="mt-0.5 text-sm text-neutral-500">
            {{ step === 1 ? 'Step 1 of 2 — pick a starting point' : 'Step 2 of 2 — name it and go' }}
          </p>
        </div>
        <div class="flex shrink-0 items-center gap-1 pt-1.5">
          <span
            class="h-1.5 w-6 rounded-full"
            :class="step >= 1 ? 'bg-red-500' : 'bg-neutral-200 dark:bg-neutral-700'"
          ></span>
          <span
            class="h-1.5 w-6 rounded-full"
            :class="step >= 2 ? 'bg-red-500' : 'bg-neutral-200 dark:bg-neutral-700'"
          ></span>
        </div>
      </div>

      <!-- Step 1 -->
      <div v-if="step === 1" class="mt-5 flex flex-col gap-2.5">
        <label
          v-for="template in store.templates"
          :key="template.id"
          class="flex cursor-pointer items-center justify-between gap-3 rounded-2xl border p-3.5 transition"
          :class="
            selectedTemplateId === template.id
              ? 'border-red-300 bg-red-50 dark:border-red-500/40 dark:bg-red-500/10'
              : 'border-neutral-200 bg-neutral-50 hover:border-neutral-300 dark:border-neutral-800 dark:bg-neutral-900/60'
          "
        >
          <input v-model="selectedTemplateId" type="radio" :value="template.id" class="sr-only" />
          <div class="flex min-w-0 items-center gap-3">
            <span
              class="flex h-4 w-4 shrink-0 items-center justify-center rounded-full border-2"
              :class="
                selectedTemplateId === template.id
                  ? 'border-red-500'
                  : 'border-neutral-300 dark:border-neutral-600'
              "
            >
              <span v-if="selectedTemplateId === template.id" class="h-2 w-2 rounded-full bg-red-500"></span>
            </span>
            <div class="min-w-0">
              <p class="font-semibold text-neutral-900 dark:text-neutral-100">{{ template.name }}</p>
              <p class="truncate text-xs text-neutral-500">{{ template.description }}</p>
            </div>
          </div>
          <BasePill variant="mono" class="shrink-0">{{ template.tag }}</BasePill>
        </label>

        <p class="mt-1 text-xs text-neutral-400">
          Node-based starters (Vue, React, ...) aren't available yet — Rezure doesn't bundle
          Node.js/npm. Let me know if that's worth adding next.
        </p>

        <div class="mt-3 flex justify-end gap-2">
          <button
            type="button"
            class="rounded-full border border-neutral-200 bg-white/70 px-5 py-2.5 text-sm font-semibold text-neutral-700 transition hover:bg-white dark:border-neutral-700 dark:bg-neutral-900/60 dark:text-neutral-200 dark:hover:bg-neutral-800"
            @click="close"
          >
            Cancel
          </button>
          <button
            type="button"
            class="rounded-full bg-red-600 px-5 py-2.5 text-sm font-semibold text-white shadow-lg shadow-red-500/40 transition hover:bg-red-500 disabled:cursor-not-allowed disabled:opacity-50"
            :disabled="!canContinue"
            @click="goToNaming"
          >
            Continue
          </button>
        </div>
      </div>

      <!-- Step 2 -->
      <div v-else class="mt-5">
        <label class="text-xs font-medium text-neutral-500">Project name</label>
        <input
          v-model="name"
          type="text"
          placeholder="my-project"
          autofocus
          class="mt-1 w-full rounded-xl border bg-white px-3.5 py-2.5 font-mono text-sm text-neutral-900 outline-none dark:bg-neutral-950 dark:text-neutral-100"
          :class="
            nameError
              ? 'border-red-400 focus:border-red-500'
              : 'border-neutral-200 focus:border-red-400 dark:border-neutral-700'
          "
        />
        <p v-if="nameError" class="mt-1.5 text-xs text-red-600 dark:text-red-400">{{ nameError }}</p>

        <div class="mt-4 grid grid-cols-2 gap-3">
          <div class="rounded-xl bg-red-50 p-3 dark:bg-red-500/10">
            <p class="text-[10px] font-semibold tracking-wide text-red-400 uppercase">Local domain</p>
            <p class="truncate font-mono text-sm text-red-600 dark:text-red-400">
              {{ name.trim() || '…' }}.test
            </p>
          </div>
          <div class="rounded-xl bg-neutral-100 p-3 dark:bg-neutral-800">
            <p class="text-[10px] font-semibold tracking-wide text-neutral-400 uppercase">Template</p>
            <p class="truncate text-sm font-semibold text-neutral-900 dark:text-neutral-100">
              {{ selectedTemplate?.name }}
            </p>
          </div>
        </div>

        <p class="mt-2 truncate font-mono text-xs text-neutral-500">
          {{ store.wwwRoot }}\{{ name.trim() || '…' }}
        </p>

        <p v-if="store.creating" class="mt-3 text-xs text-neutral-500">
          Creating project…
          <template v-if="selectedTemplateId === 'laravel'">
            Composer is resolving and downloading dependencies — this
            typically takes 3-6 minutes, longer on a slow connection. Keep
            this open.
          </template>
          <template v-else-if="selectedTemplateId === 'wordpress'">
            downloading WordPress core — usually around a minute.
          </template>
        </p>
        <p v-if="store.createError" class="mt-3 text-sm text-red-600 dark:text-red-400">
          {{ store.createError }}
        </p>

        <div class="mt-4 flex justify-end gap-2">
          <button
            type="button"
            class="rounded-full border border-neutral-200 bg-white/70 px-5 py-2.5 text-sm font-semibold text-neutral-700 transition hover:bg-white disabled:opacity-50 dark:border-neutral-700 dark:bg-neutral-900/60 dark:text-neutral-200 dark:hover:bg-neutral-800"
            :disabled="store.creating"
            @click="goBack"
          >
            Back
          </button>
          <button
            type="button"
            class="flex items-center gap-2 rounded-full bg-red-600 px-5 py-2.5 text-sm font-semibold text-white shadow-lg shadow-red-500/40 transition hover:bg-red-500 disabled:cursor-not-allowed disabled:opacity-50"
            :disabled="!canCreate || store.creating"
            @click="submit"
          >
            <svg
              v-if="store.creating"
              viewBox="0 0 24 24"
              fill="none"
              class="h-4 w-4 animate-spin"
              aria-hidden="true"
            >
              <circle cx="12" cy="12" r="9" stroke="currentColor" stroke-width="2.5" opacity="0.25" />
              <path d="M21 12a9 9 0 0 0-9-9" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" />
            </svg>
            {{ store.creating ? 'Creating…' : 'Create project' }}
          </button>
        </div>
      </div>
    </div>
  </div>
</template>
