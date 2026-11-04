<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { RouterLink } from 'vue-router'
import type { OdooInstall } from '@/types/database'
import { useOdooStore } from '@/stores/odoo'
import { usePostgresStore } from '@/stores/postgres'
import { useSettingsStore } from '@/stores/settings'
import TechIcon from '@/components/common/TechIcon.vue'
import OdooMasterPasswordCard from '@/components/databases/OdooMasterPasswordCard.vue'

/**
 * Points an installed Odoo at Rezure's PostgreSQL, and what keeps that
 * working after a restart. The work is `services::odoo`'s; this shows the
 * change before it's made — odoo.conf line by line — and asks once, with one
 * admin prompt.
 */
const props = defineProps<{ install: OdooInstall }>()
const emit = defineEmits<{ close: [] }>()

const odoo = useOdooStore()
const postgres = usePostgresStore()
const settings = useSettingsStore()

/** Re-read after each action, by folder. */
const current = computed(
  () => odoo.installs.find((i) => i.dir === props.install.dir) ?? props.install,
)
const title = computed(() => `Odoo${current.value.version ? ` ${current.value.version}` : ''}`)
const connected = computed(() => current.value.usesRezure)

/** On by default where Odoo brought its own PostgreSQL: left as it is, it
 *  takes port 5432 back at the next boot. Forced on while it runs. */
const stopBundled = ref(current.value.bundledPostgres !== null)
const bundledRunning = computed(() => current.value.bundledPostgres?.running === true)

const major = (version: string | null | undefined) => version?.split('.')[0] ?? null
/** The version Odoo's installer would have used, against the active one. */
const versionHint = computed(() => {
  const bundled = current.value.bundledPostgresVersion
  const active = current.value.rezurePostgresVersion
  if (!active) return { text: 'Install PostgreSQL on the Switch page first.', blocking: true }
  if (bundled && major(bundled) !== major(active)) {
    return {
      text: `Odoo's installer ships PostgreSQL ${bundled}; Rezure runs ${active}. ${major(bundled)}.x on the Switch page is the closest match.`,
      blocking: false,
    }
  }
  return null
})

/** odoo.conf as it is, and as connecting leaves it. */
const changes = computed(() => {
  const c = current.value
  return [
    { key: 'db_host', before: c.dbHost ?? 'localhost', after: '127.0.0.1' },
    { key: 'db_port', before: c.dbPort ?? '5432', after: '5432' },
    { key: 'db_user', before: c.dbUser ?? '-', after: 'odoo' },
    { key: 'db_password', before: '••••••', after: 'new, random' },
    { key: 'pg_path', before: c.pgPath ?? 'not set', after: c.rezurePgPath ?? '-' },
  ].map((row) => ({ ...row, changed: row.before !== row.after }))
})

/** What Connect does, in order — the last only where Odoo has its own
 *  PostgreSQL service. */
const steps = computed(() => {
  const list = [
    {
      title: 'Create the odoo login',
      detail: 'On Rezure\'s PostgreSQL, allowed to create databases. Odoo refuses "postgres".',
    },
    {
      title: 'Rewrite odoo.conf',
      detail: 'Only the lines on the left. The original is kept as odoo.conf.rezure-backup.',
    },
  ]
  if (current.value.service?.running) {
    list.push({
      title: `Restart ${current.value.service.name}`,
      detail: 'So Odoo reads the new settings.',
    })
  }
  return list
})

/** What keeps Odoo's database up from boot — each one a setting of its own,
 *  gathered here because Odoo needs both. Keeping Rezure open is taken as
 *  given: it's the app Odoo's database lives in, so no tray setting here. */
const keepAlive = computed(() => [
  {
    label: 'Start PostgreSQL when Rezure opens',
    on: settings.startOnLaunch.includes('postgres'),
    toggle: () =>
      settings.setServiceStartsOnLaunch('postgres', !settings.startOnLaunch.includes('postgres')),
  },
  {
    label: 'Start Rezure with Windows',
    on: settings.startWithWindows,
    toggle: () => settings.setStartWithWindows(!settings.startWithWindows),
  },
])
const keptAliveCount = computed(() => keepAlive.value.filter((item) => item.on).length)

async function connect() {
  await odoo.connect(current.value.dir, stopBundled.value || bundledRunning.value)
}

function close() {
  if (odoo.busy) return
  emit('close')
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') close()
}

onMounted(() => {
  window.addEventListener('keydown', onKeydown)
  if (!settings.loaded) settings.fetchAll()
  postgres.fetchVersions().catch(() => {})
})
onUnmounted(() => window.removeEventListener('keydown', onKeydown))

const SECTION_TITLE = 'text-[11px] font-semibold tracking-wide text-neutral-500 uppercase'
</script>

<template>
  <div class="glass-scrim fixed inset-0 z-50 flex items-center justify-center p-4">
    <div class="glass-strong flex max-h-[90vh] w-full max-w-4xl flex-col rounded-3xl">
      <!-- Header -->
      <div class="flex items-start gap-3.5 px-6 pt-6">
        <img
          v-if="current.icon"
          :src="current.icon"
          alt=""
          class="h-11 w-11 shrink-0 rounded-2xl shadow-sm"
        />
        <span
          v-else
          class="flex h-11 w-11 shrink-0 items-center justify-center rounded-2xl bg-linear-to-br from-accent-400 to-accent-600 text-white shadow-sm"
          aria-hidden="true"
        >
          <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            class="h-5 w-5"
          >
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              d="M21 8 12 3 3 8v8l9 5 9-5V8ZM3 8l9 5 9-5M12 13v8"
            />
          </svg>
        </span>
        <div class="min-w-0 flex-1">
          <div class="flex flex-wrap items-center gap-2">
            <h2 class="text-xl font-bold tracking-tight">{{ title }}</h2>
            <span
              class="flex items-center gap-1.5 rounded-full px-2.5 py-0.5 text-[11px] font-semibold"
              :class="
                current.loginMissing
                  ? 'bg-red-500/15 text-red-700 dark:text-red-300'
                  : connected
                    ? 'bg-emerald-500/15 text-emerald-700 dark:text-emerald-300'
                    : 'bg-amber-500/15 text-amber-700 dark:text-amber-300'
              "
            >
              <span
                class="h-1.5 w-1.5 rounded-full"
                :class="
                  current.loginMissing
                    ? 'bg-red-500'
                    : connected
                      ? 'bg-emerald-500'
                      : 'bg-amber-500'
                "
              ></span>
              {{
                current.loginMissing
                  ? 'Login missing'
                  : connected
                    ? "On Rezure's PostgreSQL"
                    : 'On its own database settings'
              }}
            </span>
          </div>
          <p class="mt-0.5 truncate font-mono text-[11px] text-neutral-500" :title="current.dir">
            {{ current.dir }}
          </p>
        </div>
        <button
          type="button"
          title="Close"
          class="glass-ghost flex h-8 w-8 shrink-0 items-center justify-center rounded-full text-neutral-500 transition disabled:opacity-40"
          :disabled="odoo.busy !== null"
          @click="close"
        >
          <svg
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            class="h-4 w-4"
          >
            <path stroke-linecap="round" d="M6 6l12 12M18 6 6 18" />
          </svg>
        </button>
      </div>

      <div class="mt-5 flex-1 overflow-y-auto px-6 pb-2">
        <!-- Connected on paper, but the login it names is gone: every page
             in Odoo fails until it's back. First, because nothing else here
             matters until then. -->
        <div
          v-if="current.loginMissing"
          class="mb-4 flex items-center gap-3 rounded-2xl border border-red-300/60 bg-red-50/70 p-3.5 dark:border-red-500/25 dark:bg-red-500/10"
        >
          <div class="min-w-0 flex-1 text-xs text-red-800 dark:text-red-200">
            <p class="text-sm font-semibold">
              The <span class="font-mono">{{ current.dbUser ?? 'odoo' }}</span> login is missing
            </p>
            <p class="mt-0.5">
              odoo.conf points at Rezure's PostgreSQL, but that login isn't there, so Odoo can't
              connect ("role does not exist"). Recreating it uses the password already in odoo.conf
              - no admin prompt.
            </p>
          </div>
          <button
            type="button"
            class="glass-accent shrink-0 rounded-full px-3.5 py-1.5 text-xs font-semibold transition disabled:opacity-50"
            :disabled="odoo.busy !== null"
            @click="odoo.repairLogin(current.dir)"
          >
            {{ odoo.busy === 'repair' ? 'Recreating…' : 'Recreate it' }}
          </button>
        </div>

        <!-- Odoo → which PostgreSQL, at a glance. -->
        <div class="glass-inset flex items-center gap-3 rounded-2xl p-3.5">
          <div class="flex min-w-0 flex-1 items-center gap-2.5">
            <img
              v-if="current.icon"
              :src="current.icon"
              alt=""
              class="h-9 w-9 shrink-0 rounded-xl"
            />
            <span
              v-else
              class="flex h-9 w-9 shrink-0 items-center justify-center rounded-xl bg-accent-500/12 text-accent-600 dark:text-accent-400"
              aria-hidden="true"
            >
              <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                class="h-4.5 w-4.5"
              >
                <path
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  d="M21 8 12 3 3 8v8l9 5 9-5V8Z"
                />
              </svg>
            </span>
            <div class="min-w-0">
              <p class="truncate text-sm font-semibold">{{ title }}</p>
              <p class="flex items-center gap-1 truncate text-[11px] text-neutral-500">
                <span
                  class="h-1.5 w-1.5 shrink-0 rounded-full"
                  :class="current.service?.running ? 'bg-emerald-500' : 'bg-neutral-400'"
                ></span>
                {{
                  current.service
                    ? `${current.service.name} · ${current.service.running ? 'running' : 'stopped'}`
                    : 'no Windows service found'
                }}
              </p>
            </div>
          </div>

          <div
            class="flex shrink-0 flex-col items-center px-1 text-[10px] font-semibold text-neutral-400"
          >
            <span class="font-mono">:5432</span>
            <svg viewBox="0 0 48 12" fill="none" class="h-3 w-12" aria-hidden="true">
              <path
                d="M1 6h44m0 0-5-4m5 4-5 4"
                stroke="currentColor"
                stroke-width="1.6"
                stroke-linecap="round"
                stroke-linejoin="round"
                :stroke-dasharray="connected ? '0' : '3 3'"
              />
            </svg>
          </div>

          <div class="flex min-w-0 flex-1 items-center gap-2.5">
            <span class="glass flex h-9 w-9 shrink-0 items-center justify-center rounded-xl">
              <TechIcon id="postgres" :size="20" />
            </span>
            <div class="min-w-0">
              <p class="truncate text-sm font-semibold">
                Rezure's PostgreSQL
                <span v-if="current.rezurePostgresVersion" class="font-normal text-neutral-500">
                  {{ current.rezurePostgresVersion }}
                </span>
              </p>
              <p
                v-if="current.bundledPostgres && !connected"
                class="truncate text-[11px] text-neutral-500"
              >
                instead of
                <span class="line-through decoration-neutral-400">{{
                  current.bundledPostgres.name
                }}</span>
                {{ current.bundledPostgresVersion ?? '' }}
              </p>
              <p v-else class="truncate text-[11px] text-neutral-500">
                {{ connected ? 'connected' : 'not connected yet' }}
              </p>
            </div>
          </div>
        </div>

        <div class="mt-5 grid gap-5 md:grid-cols-2">
          <!-- Left: what connecting changes, or what's in place. -->
          <div class="min-w-0">
            <p :class="SECTION_TITLE">{{ connected ? 'odoo.conf now' : 'Changes to odoo.conf' }}</p>
            <div class="glass-inset mt-2 overflow-hidden rounded-2xl">
              <div
                v-for="row in changes"
                :key="row.key"
                class="glass-divider grid grid-cols-[5.5rem_1fr] items-start gap-2 border-b px-3.5 py-2 text-xs last:border-b-0"
              >
                <span class="pt-px font-mono text-neutral-500">{{ row.key }}</span>
                <div v-if="connected" class="min-w-0 truncate font-mono" :title="row.before">
                  {{ row.key === 'db_password' ? '••••••' : row.before }}
                </div>
                <div v-else class="min-w-0">
                  <p
                    v-if="row.changed"
                    class="truncate font-mono text-neutral-400 line-through decoration-neutral-400/70"
                    :title="row.before"
                  >
                    {{ row.before }}
                  </p>
                  <p
                    class="truncate font-mono"
                    :class="
                      row.changed
                        ? 'font-semibold text-accent-700 dark:text-accent-300'
                        : 'text-neutral-600 dark:text-neutral-300'
                    "
                    :title="row.after"
                  >
                    {{ row.after }}
                  </p>
                </div>
              </div>
            </div>

            <OdooMasterPasswordCard v-if="connected" class="mt-4" :install="current" />

            <template v-if="!connected">
              <p :class="[SECTION_TITLE, 'mt-5']">What Connect does</p>
              <ol class="mt-2 space-y-2.5">
                <li v-for="(step, index) in steps" :key="step.title" class="flex gap-2.5">
                  <span
                    class="flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-accent-500/12 text-[11px] font-bold text-accent-700 dark:text-accent-300"
                    >{{ index + 1 }}</span
                  >
                  <div class="min-w-0">
                    <p class="text-sm font-semibold">{{ step.title }}</p>
                    <p class="text-xs text-neutral-500">{{ step.detail }}</p>
                  </div>
                </li>
                <!-- Optional, so it's a switch rather than a fixed step. -->
                <li v-if="current.bundledPostgres" class="flex gap-2.5">
                  <span
                    class="flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-accent-500/12 text-[11px] font-bold text-accent-700 dark:text-accent-300"
                    >{{ steps.length + 1 }}</span
                  >
                  <label
                    class="flex min-w-0 flex-1 cursor-pointer items-start justify-between gap-3"
                  >
                    <span class="min-w-0">
                      <span class="block text-sm font-semibold">
                        Turn off {{ current.bundledPostgres.name }}
                      </span>
                      <span class="block text-xs text-neutral-500">
                        Odoo's own PostgreSQL also wants port 5432. Stopped and set to Manual.
                        <template v-if="bundledRunning">Needed - it's running now.</template>
                      </span>
                    </span>
                    <input
                      v-model="stopBundled"
                      type="checkbox"
                      class="mt-1 h-4 w-4 shrink-0 accent-accent-600"
                      :disabled="bundledRunning"
                    />
                  </label>
                </li>
              </ol>
            </template>
          </div>

          <!-- Right: what keeps it working, and what to know. -->
          <div class="min-w-0 space-y-3">
            <div class="glass-inset rounded-2xl p-3.5">
              <div class="flex items-center justify-between gap-2">
                <p :class="SECTION_TITLE">After a restart</p>
                <span
                  class="text-[11px] font-semibold"
                  :class="
                    keptAliveCount === keepAlive.length
                      ? 'text-emerald-600 dark:text-emerald-400'
                      : 'text-amber-600 dark:text-amber-400'
                  "
                >
                  {{
                    keptAliveCount === keepAlive.length
                      ? 'All set'
                      : `${keptAliveCount} of ${keepAlive.length} on`
                  }}
                </span>
              </div>
              <p class="mt-1 text-xs text-neutral-500">
                Odoo starts with Windows and wants its database right away. Rezure's PostgreSQL runs
                only while Rezure does.
              </p>
              <div class="mt-2.5 space-y-2">
                <button
                  v-for="item in keepAlive"
                  :key="item.label"
                  type="button"
                  role="switch"
                  :aria-checked="item.on"
                  class="flex w-full items-center justify-between gap-3 text-left"
                  @click="item.toggle()"
                >
                  <span class="text-xs text-neutral-700 dark:text-neutral-200">{{
                    item.label
                  }}</span>
                  <span
                    class="relative h-5 w-9 shrink-0 rounded-full transition"
                    :class="item.on ? 'bg-accent-600' : 'bg-neutral-900/15 dark:bg-white/15'"
                  >
                    <span
                      class="absolute top-0.5 h-4 w-4 rounded-full bg-white shadow transition-all"
                      :class="item.on ? 'left-4.5' : 'left-0.5'"
                    />
                  </span>
                </button>
              </div>
            </div>

            <OdooMasterPasswordCard v-if="!connected" :install="current" />

            <div
              v-if="versionHint"
              class="flex gap-2.5 rounded-2xl p-3.5 text-xs"
              :class="
                versionHint.blocking
                  ? 'bg-red-100/60 text-red-800 dark:bg-red-500/10 dark:text-red-200'
                  : 'glass-inset text-neutral-600 dark:text-neutral-300'
              "
            >
              <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                class="mt-px h-4 w-4 shrink-0 opacity-70"
                aria-hidden="true"
              >
                <circle cx="12" cy="12" r="9" />
                <path stroke-linecap="round" d="M12 11v5M12 8h.01" />
              </svg>
              <p>
                {{ versionHint.text }}
                <RouterLink
                  to="/switch"
                  class="font-semibold text-accent-600 underline dark:text-accent-400"
                  @click="close"
                  >Open Switch</RouterLink
                >
              </p>
            </div>

            <div
              v-if="current.bundledPostgres && !connected"
              class="flex gap-2.5 rounded-2xl bg-amber-100/60 p-3.5 text-xs text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
            >
              <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                class="mt-px h-4 w-4 shrink-0"
                aria-hidden="true"
              >
                <path
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  d="M12 9v4M12 17h.01M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0Z"
                />
              </svg>
              <p>
                Databases already in Odoo's own PostgreSQL stay there, and Odoo won't list them
                after this. Back them up from Odoo's database manager first if you need them - or
                put Odoo's settings back any time.
              </p>
            </div>
          </div>
        </div>

        <p
          v-if="odoo.error"
          class="mt-4 rounded-xl bg-red-100/60 px-3.5 py-2.5 text-sm text-red-700 dark:bg-red-500/10 dark:text-red-300"
        >
          {{ odoo.error }}
        </p>
      </div>

      <!-- Footer -->
      <div
        class="glass-divider mt-3 flex flex-wrap items-center justify-between gap-2 border-t px-6 py-4"
      >
        <button
          v-if="current.canRestore"
          type="button"
          class="rounded-full px-1 py-2 text-xs font-semibold text-neutral-500 transition hover:text-neutral-800 disabled:opacity-50 dark:hover:text-neutral-200"
          :disabled="odoo.busy !== null"
          title="Puts the original odoo.conf back and turns Odoo's own PostgreSQL back on"
          @click="odoo.restore(current.dir)"
        >
          {{ odoo.busy === 'restore' ? 'Putting it back…' : "Put back Odoo's own settings" }}
        </button>
        <span v-else class="text-[11px] text-neutral-400">
          {{ connected ? '' : 'Windows asks for administrator permission once.' }}
        </span>

        <div class="flex gap-2">
          <template v-if="connected">
            <button
              type="button"
              class="glass-btn rounded-full px-4 py-2 text-sm font-semibold text-neutral-700 transition dark:text-neutral-200"
              @click="close"
            >
              Done
            </button>
            <button
              type="button"
              class="glass-accent flex items-center gap-2 rounded-full px-4 py-2 text-sm font-semibold transition"
              @click="odoo.open(current.dir)"
            >
              Open Odoo
              <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                class="h-3.5 w-3.5"
                aria-hidden="true"
              >
                <path
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  d="M14 4h6v6M20 4l-9 9M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5"
                />
              </svg>
            </button>
          </template>
          <template v-else>
            <button
              type="button"
              class="rounded-full px-4 py-2 text-sm font-semibold text-neutral-600 transition disabled:opacity-50 dark:text-neutral-300"
              :disabled="odoo.busy !== null"
              @click="close"
            >
              Cancel
            </button>
            <button
              type="button"
              class="glass-accent flex items-center gap-2 rounded-full px-4 py-2 text-sm font-semibold transition disabled:opacity-50"
              :disabled="odoo.busy !== null || versionHint?.blocking === true"
              @click="connect"
            >
              <!-- The shield says "this asks for admin" before the prompt does. -->
              <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                class="h-4 w-4"
                aria-hidden="true"
              >
                <path
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  d="M12 3 4 6v6c0 4.5 3.4 8.3 8 9 4.6-.7 8-4.5 8-9V6l-8-3Z"
                />
              </svg>
              {{ odoo.busy === 'connect' ? 'Connecting…' : 'Connect to Rezure' }}
            </button>
          </template>
        </div>
      </div>
    </div>
  </div>
</template>
