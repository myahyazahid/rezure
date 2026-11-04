<script setup lang="ts">
import { computed, ref } from 'vue'
import type { OdooInstall } from '@/types/database'
import { useOdooStore, type ServiceAction } from '@/stores/odoo'

/**
 * Odoo's master password — what its database manager asks for, and the
 * first wall right after connecting ("bad admin password") — and the
 * service controls that go with editing it. Said by state, from
 * `OdooInstall.masterPassword`; Rezure never reads the value.
 */
const props = defineProps<{ install: OdooInstall }>()

const odoo = useOdooStore()
/** Notepad is started elevated, which waits on the UAC prompt. */
const opening = ref(false)
async function openConf() {
  opening.value = true
  await odoo.openConf(props.install.dir)
  opening.value = false
}

const running = computed(() => props.install.service?.running === true)
/** Running: Stop and Restart. Stopped: Start. */
const actions = computed<{ id: ServiceAction; label: string; busyLabel: string }[]>(() =>
  running.value
    ? [
        { id: 'stop', label: 'Stop', busyLabel: 'Stopping…' },
        { id: 'restart', label: 'Restart', busyLabel: 'Restarting…' },
      ]
    : [{ id: 'start', label: 'Start', busyLabel: 'Starting…' }],
)

const BUTTON =
  'glass-btn flex shrink-0 items-center gap-1.5 rounded-md px-2.5 py-1 text-[11px] font-semibold text-accent-700 transition disabled:opacity-50 dark:text-accent-300'
</script>

<template>
  <div class="glass-inset rounded-2xl p-3.5">
    <div class="flex items-center gap-2">
      <svg
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        class="h-4 w-4 shrink-0 text-accent-600 dark:text-accent-400"
        aria-hidden="true"
      >
        <circle cx="8" cy="15" r="4" />
        <path
          stroke-linecap="round"
          stroke-linejoin="round"
          d="m10.8 12.2 9.2-9.2M17 6l3 3M14 9l2 2"
        />
      </svg>
      <p class="text-[11px] font-semibold tracking-wide text-neutral-500 uppercase">
        Odoo's Master Password
      </p>
    </div>
    <p class="mt-1.5 text-xs text-neutral-600 dark:text-neutral-300">
      Asked for by Odoo before creating, backing up or deleting a database. It's
      <span class="font-mono">admin_passwd</span> in odoo.conf - not a database login, and Rezure
      leaves it as it is.
    </p>

    <p v-if="install.masterPassword === 'missing'" class="mt-2 text-xs">
      Not set, so it's <span class="font-mono font-semibold">admin</span>.
    </p>
    <p v-else-if="install.masterPassword === 'plainText'" class="mt-2 text-xs">
      Written in odoo.conf as plain text - open it below to read it.
    </p>
    <template v-else>
      <p class="mt-2 text-xs">
        <span class="font-semibold">Stored hashed</span> - it can't be decrypted or read back, only
        replaced. Forgotten it?
      </p>
      <ol
        class="mt-1.5 list-decimal space-y-0.5 pl-5 text-xs text-neutral-700 dark:text-neutral-200"
      >
        <li>Open odoo.conf below - Notepad opens as administrator, so it can save.</li>
        <li>
          Replace everything after <span class="font-mono">admin_passwd =</span> with a new
          password.
        </li>
        <li>Save, then click <span class="font-semibold">Restart</span> below.</li>
      </ol>
    </template>
    <p v-if="install.masterPassword !== 'hashed'" class="mt-1 text-xs text-neutral-500">
      Changed it? Click <span class="font-semibold">Restart</span> below so Odoo reads it.
    </p>

    <!-- The file, on its own row. -->
    <div
      class="mt-2.5 flex items-center gap-1.5 rounded-lg bg-black/4 py-1 pr-1 pl-2.5 dark:bg-white/5"
    >
      <span
        class="min-w-0 flex-1 truncate font-mono text-[11px] text-neutral-600 dark:text-neutral-300"
        :title="install.confPath"
        >{{ install.confPath }}</span
      >
      <button
        type="button"
        :class="BUTTON"
        :disabled="opening"
        title="Opens odoo.conf in Notepad as administrator - Windows asks first"
        @click="openConf"
      >
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          class="h-3 w-3"
          aria-hidden="true"
        >
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            d="M12 3 4 6v6c0 4.5 3.4 8.3 8 9 4.6-.7 8-4.5 8-9V6l-8-3Z"
          />
        </svg>
        {{ opening ? 'Opening…' : 'Open' }}
      </button>
    </div>

    <!-- Odoo's service, below it: what reading the file again takes. -->
    <div
      v-if="install.service"
      class="mt-1.5 flex items-center gap-1.5 rounded-lg bg-black/4 py-1 pr-1 pl-2.5 dark:bg-white/5"
    >
      <span
        class="h-1.5 w-1.5 shrink-0 rounded-full"
        :class="running ? 'bg-emerald-500' : 'bg-neutral-400'"
        aria-hidden="true"
      ></span>
      <span
        class="min-w-0 flex-1 truncate font-mono text-[11px] text-neutral-600 dark:text-neutral-300"
        :title="install.service.name"
      >
        {{ install.service.name }} · {{ running ? 'running' : 'stopped' }}
      </span>
      <button
        v-for="action in actions"
        :key="action.id"
        type="button"
        :class="BUTTON"
        :disabled="odoo.busy !== null"
        :title="
          install.controlAllowed
            ? `${action.label} ${install.service.name}`
            : `${action.label} ${install.service.name} - Windows asks first`
        "
        @click="odoo.control(install.dir, action.id)"
      >
        <!-- Stop: a square. Restart: the circular arrows. Start: a play mark. -->
        <svg v-if="action.id === 'stop'" viewBox="0 0 10 10" class="h-2 w-2" aria-hidden="true">
          <rect width="10" height="10" rx="1.5" fill="currentColor" />
        </svg>
        <svg
          v-else-if="action.id === 'start'"
          viewBox="0 0 10 10"
          class="h-2.5 w-2.5"
          aria-hidden="true"
        >
          <path d="M1.5 0.8 9 5 1.5 9.2Z" fill="currentColor" />
        </svg>
        <svg
          v-else
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          class="h-3 w-3"
          :class="odoo.busy === 'restart' ? 'animate-spin' : ''"
          aria-hidden="true"
        >
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            d="M4.5 12a7.5 7.5 0 0 1 12.8-5.3L20 9M20 9V4M20 9h-5M19.5 12a7.5 7.5 0 0 1-12.8 5.3L4 15m0 0v5m0-5h5"
          />
        </svg>
        {{ odoo.busy === action.id ? action.busyLabel : action.label }}
      </button>
    </div>

    <!-- Each control asks for admin until this is done once. -->
    <p
      v-if="install.service && !install.controlAllowed"
      class="mt-1.5 flex flex-wrap items-center gap-x-1.5 text-[11px] text-neutral-500"
    >
      Each of these asks for admin.
      <button
        type="button"
        class="font-semibold text-accent-600 underline-offset-2 transition hover:underline disabled:opacity-50 dark:text-accent-400"
        :disabled="odoo.busy !== null"
        :title="`Lets your Windows account start and stop ${install.service.name} without an admin prompt - this service only. Putting back Odoo's own settings removes it.`"
        @click="odoo.allowControl(install.dir)"
      >
        {{ odoo.busy === 'allow' ? 'Allowing…' : 'Allow without asking' }}
      </button>
    </p>
    <p
      v-else-if="install.service"
      class="mt-1.5 text-[11px] text-emerald-700 dark:text-emerald-300"
    >
      Start, Stop and Restart don't ask for admin on this account.
    </p>

    <p v-if="odoo.notice" class="mt-2 text-xs font-medium text-emerald-700 dark:text-emerald-300">
      {{ odoo.notice }}
    </p>
  </div>
</template>
