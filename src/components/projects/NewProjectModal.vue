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
    class="glass-scrim fixed inset-0 z-50 flex items-center justify-center p-4"
    @click.self="close"
  >
    <div class="glass-strong w-full max-w-md rounded-3xl p-6">
      <div class="flex items-start justify-between gap-4">
        <div>
          <h2 class="text-xl font-bold tracking-tight">New project</h2>
          <p class="mt-0.5 text-sm text-neutral-500">
            {{
              step === 1 ? 'Step 1 of 2 — pick a starting point' : 'Step 2 of 2 — name it and go'
            }}
          </p>
        </div>
        <div class="flex shrink-0 items-center gap-1 pt-1.5">
          <span
            class="h-1.5 w-6 rounded-full"
            :class="step >= 1 ? 'bg-accent-500' : 'bg-neutral-900/10 dark:bg-white/10'"
          ></span>
          <span
            class="h-1.5 w-6 rounded-full"
            :class="step >= 2 ? 'bg-accent-500' : 'bg-neutral-900/10 dark:bg-white/10'"
          ></span>
        </div>
      </div>

      <!-- Step 1 -->
      <div v-if="step === 1" class="mt-5 flex flex-col gap-2.5">
        <label
          v-for="template in store.templates"
          :key="template.id"
          class="flex cursor-pointer items-center justify-between gap-3 rounded-2xl p-3.5 transition"
          :class="
            selectedTemplateId === template.id
              ? 'glass-selected'
              : 'glass-inset hover:bg-white/70 dark:hover:bg-white/8'
          "
        >
          <input v-model="selectedTemplateId" type="radio" :value="template.id" class="sr-only" />
          <div class="flex min-w-0 items-center gap-3">
            <span
              class="flex h-4 w-4 shrink-0 items-center justify-center rounded-full border-2"
              :class="
                selectedTemplateId === template.id
                  ? 'border-accent-500'
                  : 'border-neutral-900/20 dark:border-white/20'
              "
            >
              <span
                v-if="selectedTemplateId === template.id"
                class="h-2 w-2 rounded-full bg-accent-500"
              ></span>
            </span>
            <div class="min-w-0">
              <p class="font-semibold text-neutral-900 dark:text-neutral-100">
                {{ template.name }}
              </p>
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
            class="glass-btn rounded-full px-4 py-2 text-sm font-semibold text-neutral-700 transition dark:text-neutral-200"
            @click="close"
          >
            Cancel
          </button>
          <button
            type="button"
            class="glass-accent rounded-full px-4 py-2 text-sm font-semibold transition disabled:cursor-not-allowed disabled:opacity-50"
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
          class="glass-inset mt-1 w-full rounded-xl px-3.5 py-2.5 font-mono text-sm text-neutral-900 outline-none dark:text-neutral-100"
          :class="nameError ? 'border-red-400 focus:border-red-500' : 'focus:border-accent-400/70'"
        />
        <p v-if="nameError" class="mt-1.5 text-xs text-red-600 dark:text-red-400">
          {{ nameError }}
        </p>

        <div class="mt-4 grid grid-cols-2 gap-3">
          <div class="glass-selected rounded-xl p-3">
            <p class="text-[10px] font-semibold tracking-wide text-accent-400 uppercase">
              Local domain
            </p>
            <p class="truncate font-mono text-sm text-accent-600 dark:text-accent-400">
              {{ name.trim() || '…' }}.test
            </p>
          </div>
          <div class="glass-inset rounded-xl p-3">
            <p class="text-[10px] font-semibold tracking-wide text-neutral-400 uppercase">
              Template
            </p>
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
            Composer is resolving and downloading dependencies — this typically takes 3-6 minutes,
            longer on a slow connection. Keep this open.
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
            class="glass-btn rounded-full px-4 py-2 text-sm font-semibold text-neutral-700 transition disabled:opacity-50 dark:text-neutral-200"
            :disabled="store.creating"
            @click="goBack"
          >
            Back
          </button>
          <button
            type="button"
            class="glass-accent flex items-center gap-2 rounded-full px-4 py-2 text-sm font-semibold transition disabled:cursor-not-allowed disabled:opacity-50"
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
              <circle
                cx="12"
                cy="12"
                r="9"
                stroke="currentColor"
                stroke-width="2.5"
                opacity="0.25"
              />
              <path
                d="M21 12a9 9 0 0 0-9-9"
                stroke="currentColor"
                stroke-width="2.5"
                stroke-linecap="round"
              />
            </svg>
            {{ store.creating ? 'Creating…' : 'Create project' }}
          </button>
        </div>
      </div>
    </div>
  </div>
</template>
