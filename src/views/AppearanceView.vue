<script setup lang="ts">
import { computed, ref } from 'vue'
import {
  ADJUSTMENTS,
  THEMES,
  THEME_CATEGORIES,
  UI_SCALES,
  useAppearanceStore,
  type AdjustmentKey,
  type ThemeCategory,
} from '@/stores/appearance'

const appearance = useAppearanceStore()

// Light/dark is switched from the title bar, so this page is only about the
// look itself.
const SLIDERS: readonly {
  key: AdjustmentKey
  label: string
  hint: string
  low: string
  high: string
}[] = [
  {
    key: 'brightness',
    label: 'Brightness',
    hint: 'The whole window.',
    low: 'Dimmer',
    high: 'Brighter',
  },
  {
    key: 'saturation',
    label: 'Colour intensity',
    hint: 'How vivid the theme colours and backdrop are.',
    low: 'Muted',
    high: 'Vivid',
  },
  {
    key: 'glassSolidity',
    label: 'Glass opacity',
    hint: 'More solid panels are easier to read over a busy backdrop.',
    low: 'As designed',
    high: 'Solid',
  },
]

function onSlider(key: AdjustmentKey, e: Event, save: boolean) {
  appearance.setAdjustment(key, Number((e.target as HTMLInputElement).value), save)
}

const category = ref<ThemeCategory | 'All'>('All')
const categoryTabs = computed(() => ['All' as const, ...THEME_CATEGORIES])

const visibleThemes = computed(() =>
  category.value === 'All' ? THEMES : THEMES.filter((t) => t.category === category.value),
)
</script>

<template>
  <section>
    <h1 class="text-2xl font-semibold text-neutral-900 dark:text-neutral-100">Appearance</h1>
    <p class="mt-1 text-sm text-neutral-500">
      Theme, decoration and how the window looks. Changes apply right away.
    </p>

    <p v-if="appearance.error" class="mt-3 text-sm text-red-600 dark:text-red-400">
      Couldn't save your choice: {{ appearance.error }}
    </p>

    <div class="mt-6">
      <div class="flex flex-wrap items-end justify-between gap-3">
        <div>
          <h2 class="text-sm font-semibold text-neutral-900 dark:text-neutral-100">Theme</h2>
          <p class="mt-0.5 text-xs text-neutral-500">Accent colour, backdrop and decoration.</p>
        </div>

        <div class="glass-inset flex gap-0.5 rounded-full p-0.5">
          <button
            v-for="tab in categoryTabs"
            :key="tab"
            type="button"
            class="rounded-full px-3 py-1 text-xs font-semibold transition"
            :class="
              category === tab
                ? 'glass-raised text-neutral-900 dark:text-neutral-100'
                : 'glass-ghost'
            "
            @click="category = tab"
          >
            {{ tab }}
          </button>
        </div>
      </div>

      <div
        role="radiogroup"
        aria-label="Theme"
        class="mt-3 grid gap-3 sm:grid-cols-2 xl:grid-cols-3"
      >
        <button
          v-for="t in visibleThemes"
          :key="t.id"
          type="button"
          role="radio"
          :aria-checked="appearance.theme === t.id"
          class="rounded-2xl p-2 text-left transition"
          :class="appearance.theme === t.id ? 'glass-selected' : 'glass glass-hover'"
          @click="appearance.setTheme(t.id)"
        >
          <!-- A miniature of the app wearing this theme. `data-theme` on the
               wrapper is all it takes: the theme's tokens are scoped to it,
               so the glass, accent and backdrop inside are the real ones. -->
          <div
            :data-theme="t.id"
            aria-hidden="true"
            class="bg-app flex h-32 gap-2 overflow-hidden rounded-xl p-2"
          >
            <div class="glass flex w-1/3 flex-col gap-1 rounded-lg p-1.5">
              <div class="glass-selected flex items-center gap-1 rounded-md px-1 py-1">
                <span class="h-2 w-2 rounded-sm bg-accent-500"></span>
                <span class="h-1 flex-1 rounded-full bg-neutral-900/25 dark:bg-white/40"></span>
              </div>
              <div v-for="i in 3" :key="i" class="flex items-center gap-1 px-1 py-1">
                <span class="glass-inset h-2 w-2 rounded-sm"></span>
                <span class="h-1 flex-1 rounded-full bg-neutral-900/15 dark:bg-white/20"></span>
              </div>
            </div>

            <div class="flex flex-1 flex-col gap-2">
              <div class="glass flex items-center justify-between gap-2 rounded-lg p-2">
                <span class="h-1.5 w-1/2 rounded-full bg-neutral-900/25 dark:bg-white/40"></span>
                <span class="relative h-3 w-5 shrink-0 rounded-full bg-accent-600">
                  <span class="absolute top-0.5 right-0.5 h-2 w-2 rounded-full bg-white"></span>
                </span>
              </div>
              <div class="glass flex flex-1 flex-col justify-end gap-1.5 rounded-lg p-2">
                <span class="h-1 w-2/3 rounded-full bg-neutral-900/15 dark:bg-white/20"></span>
                <span class="h-1.5 overflow-hidden rounded-full bg-accent-500/15">
                  <span
                    class="block h-full w-3/5 rounded-full bg-linear-to-r from-accent-500 to-accent-alt"
                  ></span>
                </span>
              </div>
            </div>
          </div>

          <div class="flex items-start justify-between gap-2 px-1.5 pt-2.5 pb-1">
            <div class="min-w-0">
              <p class="flex items-center gap-2">
                <span class="font-semibold text-neutral-900 dark:text-neutral-100">
                  {{ t.name }}
                </span>
                <span
                  class="glass-inset rounded-full px-2 py-0.5 text-[10px] font-semibold tracking-wide text-neutral-500 uppercase dark:text-neutral-400"
                >
                  {{ t.category }}
                </span>
              </p>
              <p class="mt-0.5 text-xs text-neutral-500">{{ t.description }}</p>
            </div>
            <svg
              v-if="appearance.theme === t.id"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2.5"
              class="mt-0.5 h-4 w-4 shrink-0"
            >
              <path stroke-linecap="round" stroke-linejoin="round" d="m5 12.5 4.5 4.5L19 7.5" />
            </svg>
          </div>
        </button>
      </div>

      <div class="glass mt-3 flex items-center justify-between gap-4 rounded-2xl p-4">
        <div>
          <p class="font-semibold text-neutral-900 dark:text-neutral-100">
            Show background decoration
          </p>
          <p class="mt-0.5 text-xs text-neutral-500">
            The theme's pattern behind the glass. Its colours stay either way.
          </p>
        </div>
        <button
          type="button"
          role="switch"
          :aria-checked="appearance.showDecoration"
          class="relative h-6 w-11 shrink-0 rounded-full transition"
          :class="
            appearance.showDecoration ? 'bg-accent-600' : 'bg-neutral-900/15 dark:bg-white/15'
          "
          @click="appearance.setShowDecoration(!appearance.showDecoration)"
        >
          <span
            class="absolute top-0.5 h-5 w-5 rounded-full bg-white shadow transition"
            :class="appearance.showDecoration ? 'left-5' : 'left-0.5'"
          />
        </button>
      </div>
    </div>

    <div class="mt-8">
      <div class="flex flex-wrap items-end justify-between gap-3">
        <div>
          <h2 class="text-sm font-semibold text-neutral-900 dark:text-neutral-100">Adjustments</h2>
          <p class="mt-0.5 text-xs text-neutral-500">Fine-tune any theme. Applies right away.</p>
        </div>
        <button
          type="button"
          class="glass-btn rounded-full px-3 py-1.5 text-xs font-semibold transition disabled:opacity-50"
          :disabled="!appearance.isAdjusted"
          @click="appearance.resetAdjustments()"
        >
          Reset to default
        </button>
      </div>

      <div class="glass mt-3 divide-y divide-neutral-900/8 rounded-2xl dark:divide-white/8">
        <div v-for="slider in SLIDERS" :key="slider.key" class="p-4">
          <div class="flex items-baseline justify-between gap-4">
            <div>
              <p class="font-semibold text-neutral-900 dark:text-neutral-100">{{ slider.label }}</p>
              <p class="mt-0.5 text-xs text-neutral-500">{{ slider.hint }}</p>
            </div>
            <span class="font-mono text-sm text-neutral-600 dark:text-neutral-300">
              {{ appearance[slider.key] }}%
            </span>
          </div>
          <input
            type="range"
            :aria-label="slider.label"
            :min="ADJUSTMENTS[slider.key].min"
            :max="ADJUSTMENTS[slider.key].max"
            :step="ADJUSTMENTS[slider.key].step"
            :value="appearance[slider.key]"
            class="mt-3 w-full"
            @input="onSlider(slider.key, $event, false)"
            @change="onSlider(slider.key, $event, true)"
          />
          <div class="mt-1 flex justify-between text-[11px] text-neutral-400">
            <span>{{ slider.low }}</span>
            <span>{{ slider.high }}</span>
          </div>
        </div>

        <div class="flex flex-wrap items-center justify-between gap-4 p-4">
          <div>
            <p class="font-semibold text-neutral-900 dark:text-neutral-100">Interface size</p>
            <p class="mt-0.5 text-xs text-neutral-500">
              Text and controls, like zooming the window.
            </p>
          </div>
          <div class="glass-inset flex gap-0.5 rounded-full p-0.5">
            <button
              v-for="scale in UI_SCALES"
              :key="scale"
              type="button"
              class="rounded-full px-3 py-1 text-xs font-semibold transition"
              :class="
                appearance.uiScale === scale
                  ? 'glass-raised text-neutral-900 dark:text-neutral-100'
                  : 'glass-ghost'
              "
              @click="appearance.setUiScale(scale)"
            >
              {{ scale }}%
            </button>
          </div>
        </div>

        <div class="flex items-center justify-between gap-4 p-4">
          <div>
            <p class="font-semibold text-neutral-900 dark:text-neutral-100">Reduce motion</p>
            <p class="mt-0.5 text-xs text-neutral-500">Turns off transitions and animations.</p>
          </div>
          <button
            type="button"
            role="switch"
            :aria-checked="appearance.reduceMotion"
            class="relative h-6 w-11 shrink-0 rounded-full transition"
            :class="
              appearance.reduceMotion ? 'bg-accent-600' : 'bg-neutral-900/15 dark:bg-white/15'
            "
            @click="appearance.setReduceMotion(!appearance.reduceMotion)"
          >
            <span
              class="absolute top-0.5 h-5 w-5 rounded-full bg-white shadow transition"
              :class="appearance.reduceMotion ? 'left-5' : 'left-0.5'"
            />
          </button>
        </div>
      </div>
    </div>
  </section>
</template>
