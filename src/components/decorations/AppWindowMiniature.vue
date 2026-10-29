<script setup lang="ts">
/**
 * A skeleton of the Rezure window (title bar, sidebar, Services page) for
 * the Decorations preview.
 *
 * Drawn at the real window's size in real pixels, with the same spacing as
 * App.vue, AppTitleBar, AppSidebar, DashboardView and ServiceRow, then
 * scaled down as a whole. That keeps it in proportion with the window a
 * sticker will land on, and keeping it current is a matter of copying
 * classes: when one of those components changes its layout, change the
 * matching block here.
 */
defineProps<{
  /** The window's size in CSS pixels. */
  width: number
  height: number
  /** Preview width ÷ window width. */
  scale: number
}>()

// Label lengths for the sidebar rows, so the skeleton doesn't read as one
// repeated line. One per entry in AppSidebar's `navItems`.
const NAV_LABELS = [58, 56, 64, 44, 92, 34, 58, 66, 46, 72, 74, 52]
// Which nav rows carry a count badge (Services, Projects, Databases, Switch).
const NAV_BADGES = new Set([0, 1, 2, 3])

const SERVICES = [
  { name: 96, running: true },
  { name: 72, running: true },
  { name: 64, running: true },
  { name: 104, running: false },
  { name: 80, running: false },
  { name: 88, running: false },
]

const BAR = 'rounded-full bg-neutral-900/25 dark:bg-white/35'
const BAR_SOFT = 'rounded-full bg-neutral-900/12 dark:bg-white/18'
</script>

<template>
  <div
    aria-hidden="true"
    inert
    class="bg-app pointer-events-none absolute top-0 left-0 flex origin-top-left flex-col"
    :style="{ width: `${width}px`, height: `${height}px`, transform: `scale(${scale})` }"
  >
    <!-- AppTitleBar -->
    <div class="flex h-12 shrink-0 items-center gap-3 pr-4">
      <div class="ml-3 flex w-60 shrink-0 translate-y-1.5 items-center justify-center gap-2">
        <span class="h-4 w-2.5 rounded-[3px] bg-[#ea0619]"></span>
        <span class="-ml-1.5 h-2.5 w-11 rounded-full bg-[#ea0619]/70"></span>
        <span :class="BAR_SOFT" class="h-2 w-3.5"></span>
        <span class="h-2.5 w-16 rounded-full bg-red-600/50 dark:bg-red-500/60"></span>
        <span class="glass-inset h-4.5 w-11 rounded-md"></span>
      </div>

      <div class="flex-1"></div>

      <span class="glass-btn flex h-7 w-17 items-center gap-1.5 rounded-full pl-1">
        <span class="h-5 w-5 rounded-full bg-accent-500/15 dark:bg-white/10"></span>
        <span :class="BAR" class="h-1.5 w-7"></span>
      </span>
      <div class="flex shrink-0 items-center gap-1 text-neutral-500 dark:text-neutral-400">
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          class="m-2 h-3 w-3"
        >
          <path stroke-linecap="round" d="M5 12h14" />
        </svg>
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          class="m-2 h-3 w-3"
        >
          <rect x="4.5" y="4.5" width="15" height="15" rx="2.5" />
        </svg>
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          class="m-2 h-3 w-3"
        >
          <path stroke-linecap="round" d="M6 6l12 12M18 6L6 18" />
        </svg>
      </div>
    </div>

    <div class="flex min-h-0 flex-1">
      <!-- AppSidebar -->
      <div class="glass mb-3 ml-3 flex w-60 shrink-0 flex-col gap-3 rounded-3xl p-3">
        <div class="flex min-h-0 flex-col gap-0.5 overflow-hidden">
          <div
            v-for="(label, i) in NAV_LABELS"
            :key="i"
            class="flex shrink-0 items-center gap-3 rounded-xl border px-2.5 py-1.5"
            :class="i === 0 ? 'glass-selected' : 'border-transparent'"
          >
            <span
              class="flex h-7 w-7 shrink-0 items-center justify-center rounded-lg"
              :class="i === 0 ? 'glass-accent' : 'glass-inset'"
            >
              <span
                class="h-3 w-3 rounded-[4px]"
                :class="i === 0 ? 'bg-accent-500' : 'bg-neutral-900/15 dark:bg-white/20'"
              ></span>
            </span>
            <span class="flex-1">
              <span
                class="block h-2 rounded-full"
                :class="i === 0 ? BAR : BAR_SOFT"
                :style="{ width: `${label}px` }"
              ></span>
            </span>
            <span
              v-if="NAV_BADGES.has(i)"
              class="h-2 w-5 rounded-full"
              :class="i === 0 ? 'bg-accent-500/40' : 'bg-neutral-900/12 dark:bg-white/18'"
            ></span>
          </div>
        </div>

        <!-- "Services up" card -->
        <div class="glass-inset mt-auto shrink-0 rounded-2xl p-3">
          <div class="flex items-center gap-3">
            <svg viewBox="0 0 40 40" class="h-10 w-10 shrink-0 -rotate-90">
              <circle
                cx="20"
                cy="20"
                r="16"
                fill="none"
                stroke="currentColor"
                stroke-width="4"
                class="text-neutral-900/10 dark:text-white/10"
              />
              <circle
                cx="20"
                cy="20"
                r="16"
                fill="none"
                stroke="currentColor"
                stroke-width="4"
                stroke-linecap="round"
                stroke-dasharray="100.5"
                stroke-dashoffset="50"
                class="text-emerald-500"
              />
            </svg>
            <div class="flex flex-col gap-2">
              <span :class="BAR" class="h-2.5 w-20"></span>
              <span :class="BAR_SOFT" class="h-2 w-24"></span>
            </div>
          </div>
          <div class="glass-divider mt-2.5 flex items-center justify-between border-t pt-3">
            <span :class="BAR_SOFT" class="h-2 w-12"></span>
            <span :class="BAR" class="h-2 w-14"></span>
          </div>
        </div>
      </div>

      <!-- DashboardView -->
      <div class="min-w-0 flex-1 overflow-hidden px-6 pt-5 pb-8">
        <div class="flex items-start justify-between gap-4">
          <div class="pt-1">
            <span :class="BAR" class="block h-6 w-32"></span>
            <span :class="BAR_SOFT" class="mt-3 block h-2.5 w-52"></span>
          </div>
          <div class="flex shrink-0 items-center gap-2">
            <span class="glass flex h-10 w-42 items-center gap-2 rounded-full px-4">
              <span class="h-3.5 w-3.5 rounded-full bg-neutral-900/20 dark:bg-white/25"></span>
              <span :class="BAR" class="h-2 flex-1"></span>
            </span>
            <span class="glass flex items-center gap-1 rounded-full p-1">
              <span class="glass-raised flex h-8 w-24 items-center gap-2 rounded-full px-4">
                <span class="h-2.5 w-2.5 rounded-[2px] bg-emerald-500"></span>
                <span :class="BAR" class="h-2 flex-1"></span>
              </span>
              <span class="flex h-8 w-27 items-center gap-2 px-4">
                <span class="h-3 w-3 rounded-full bg-neutral-900/20 dark:bg-white/25"></span>
                <span :class="BAR_SOFT" class="h-2 flex-1"></span>
              </span>
              <span class="flex h-8 w-24 items-center gap-2 px-4">
                <span class="h-2 w-2 rounded-[2px] bg-red-500"></span>
                <span :class="BAR_SOFT" class="h-2 flex-1"></span>
              </span>
            </span>
          </div>
        </div>

        <!-- ServiceRow -->
        <div class="mt-5 flex flex-col gap-2.5">
          <div
            v-for="(s, i) in SERVICES"
            :key="i"
            class="glass flex items-center gap-3 rounded-2xl p-3.5"
          >
            <span
              class="glass-inset flex h-9 w-9 shrink-0 items-center justify-center rounded-full"
              :class="s.running ? '' : 'opacity-50'"
            >
              <span class="h-4 w-4 rounded-md bg-accent-500/60"></span>
            </span>
            <div class="flex min-w-0 flex-1 flex-col gap-2">
              <div class="flex items-center gap-2">
                <span :class="BAR" class="h-2.5" :style="{ width: `${s.name}px` }"></span>
                <span class="glass-inset h-4.5 w-16 rounded-full"></span>
              </div>
              <div class="flex items-center gap-1.5">
                <span
                  class="h-1.5 w-1.5 rounded-full"
                  :class="s.running ? 'bg-emerald-500' : 'bg-neutral-400'"
                ></span>
                <span :class="BAR_SOFT" class="h-2 w-14"></span>
              </div>
            </div>
            <svg
              v-if="s.running"
              viewBox="0 0 64 20"
              fill="none"
              class="hidden h-5 w-16 shrink-0 text-accent-500 lg:block xl:w-30"
              preserveAspectRatio="none"
            >
              <path
                d="M0 15 L8 13 L16 16 L24 9 L32 12 L40 6 L48 11 L56 8 L64 10"
                stroke="currentColor"
                stroke-width="1.5"
                stroke-linejoin="round"
                vector-effect="non-scaling-stroke"
              />
            </svg>
            <span class="glass-inset h-6 w-14 shrink-0 rounded-full"></span>
            <span class="glass-inset h-6 w-12 shrink-0 rounded-full"></span>
            <span class="glass-inset flex shrink-0 items-center gap-0.5 rounded-full p-1">
              <span class="glass-raised flex h-7 w-17 items-center gap-1.5 rounded-full px-3">
                <span
                  class="h-2 w-2 rounded-[2px]"
                  :class="s.running ? 'bg-red-500' : 'bg-emerald-500'"
                ></span>
                <span :class="BAR" class="h-1.5 flex-1"></span>
              </span>
              <span class="flex h-7 w-7 items-center justify-center">
                <span class="h-3 w-3 rounded-full bg-neutral-900/15 dark:bg-white/20"></span>
              </span>
              <span class="flex h-7 w-7 items-center justify-center">
                <span class="h-1 w-3 rounded-full bg-neutral-900/15 dark:bg-white/20"></span>
              </span>
            </span>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
