<script setup lang="ts">
import { computed, reactive, watch } from 'vue'
import { open as openFileDialog } from '@tauri-apps/plugin-dialog'
import { useDbConnectionsStore, type ConnectionDraft } from '@/stores/dbConnections'
import { ENGINE_LABEL } from '@/types/dbProfile'
import type { DbEngine } from '@/types/dbProfile'
import type { TlsMode } from '@/types/dbConnection'

const emit = defineEmits<{ close: [] }>()
const store = useDbConnectionsStore()

const draft = reactive<ConnectionDraft>({
  name: '',
  host: '',
  port: 3306,
  user: 'root',
  password: '',
  engine: 'mysql',
  tlsMode: 'preferred',
  // Read-only by default: this connects to servers Rezure doesn't own, and
  // the cost of a wrong write on one of those is unbounded.
  readOnly: true,
  savePassword: true,
  useSsh: false,
  sshHost: '',
  sshPort: 22,
  sshUser: '',
  sshAuth: 'key',
  sshKeyPath: '',
  sshPassword: '',
})

async function pickKey() {
  const picked = await openFileDialog({
    multiple: false,
    directory: false,
    filters: [
      { name: 'Private key', extensions: ['pem', 'key', 'ppk'] },
      { name: 'All files', extensions: ['*'] },
    ],
  })
  if (typeof picked === 'string') draft.sshKeyPath = picked
}

/** Turning the tunnel on changes what the Host box means, so it also gets a
 *  default that matches: a tunnelled database is nearly always one that only
 *  listens on its own loopback, which is the reason it needs a tunnel. */
watch(
  () => draft.useSsh,
  (on) => {
    if (on && (draft.host === '' || draft.host === '127.0.0.1')) draft.host = '127.0.0.1'
  },
)

const ENGINES: DbEngine[] = ['mysql', 'mariadb']
const TLS_MODES: { value: TlsMode; label: string; hint: string }[] = [
  { value: 'preferred', label: 'Automatic', hint: 'Use TLS when the server offers it.' },
  { value: 'required', label: 'Required', hint: "Refuse to connect if the server won't." },
  { value: 'disabled', label: 'Off', hint: 'For a private network or an SSH tunnel.' },
]

const complete = computed(() => {
  const base = draft.name.trim() !== '' && draft.host.trim() !== '' && draft.user.trim() !== ''
  if (!draft.useSsh) return base
  const credential =
    draft.sshAuth === 'key' ? draft.sshKeyPath.trim() !== '' : draft.sshPassword !== ''
  return base && draft.sshHost.trim() !== '' && draft.sshUser.trim() !== '' && credential
})

/** The mistake worth catching in the form rather than on connect: a
 *  password typed into the key box. A key is a file, so anything without a
 *  separator can't be one. */
const keyLooksLikeAPassword = computed(
  () =>
    draft.sshAuth === 'key' &&
    draft.sshKeyPath.trim() !== '' &&
    !/[\\/]/.test(draft.sshKeyPath.trim()),
)

/** Saving is gated on a green test, not just a filled-in form.
 *
 *  Every remote failure — unreachable host, wrong port, bad credentials, a
 *  TLS requirement, a client that can't do the server's auth plugin — shows
 *  up on connect. Letting a connection be saved untested moves all of that
 *  to the moment the user tries to export something, where it reads as a
 *  broken feature rather than a wrong detail in this form. */
const canSave = computed(() => complete.value && store.testResult !== null)

/** Any edit invalidates the previous result: a test that passed against a
 *  different host says nothing about this one. */
watch(
  () => ({ ...draft }),
  () => {
    store.testResult = null
    store.testError = null
  },
  { deep: true },
)

async function submit() {
  if (!canSave.value) return
  const ok = await store.add(draft)
  if (ok) emit('close')
}

const INPUT_CLASS =
  'mt-1 w-full rounded-xl border border-neutral-200 bg-white px-3.5 py-2.5 text-sm text-neutral-900 outline-none transition focus:border-red-400 dark:border-neutral-700 dark:bg-neutral-950 dark:text-neutral-100'
const LABEL_CLASS = 'block text-xs font-medium text-neutral-500'
</script>

<template>
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
    @click.self="emit('close')"
  >
    <div
      class="max-h-[88vh] w-full max-w-lg overflow-y-auto rounded-2xl border border-neutral-200 bg-white p-6 shadow-2xl dark:border-neutral-700 dark:bg-neutral-900"
    >
      <h2 class="text-lg font-bold text-neutral-900 dark:text-neutral-100">Add connection</h2>
      <p class="mt-1 text-sm text-neutral-500">
        A server Rezure talks to but doesn't run — staging, a VPS, a shared instance. Rezure never
        starts or stops it; it only lists, exports and imports.
      </p>

      <label class="mt-5 block">
        <span :class="LABEL_CLASS">Name</span>
        <input v-model="draft.name" type="text" placeholder="Staging" :class="INPUT_CLASS" />
      </label>

      <div class="mt-4 flex gap-3">
        <label class="block flex-1">
          <span :class="LABEL_CLASS">Host</span>
          <input
            v-model="draft.host"
            type="text"
            placeholder="db.example.com"
            :class="[INPUT_CLASS, 'font-mono']"
          />
        </label>
        <label class="block w-28">
          <span :class="LABEL_CLASS">Port</span>
          <input
            v-model.number="draft.port"
            type="number"
            min="1"
            max="65535"
            :class="[INPUT_CLASS, 'font-mono']"
          />
        </label>
      </div>

      <div class="mt-4 flex gap-3">
        <label class="block flex-1">
          <span :class="LABEL_CLASS">User</span>
          <input v-model="draft.user" type="text" :class="[INPUT_CLASS, 'font-mono']" />
        </label>
        <label class="block flex-1">
          <span :class="LABEL_CLASS">Password</span>
          <input
            v-model="draft.password"
            type="password"
            placeholder="••••••••"
            :class="INPUT_CLASS"
          />
        </label>
      </div>

      <label class="mt-5 flex items-start gap-2.5">
        <input v-model="draft.useSsh" type="checkbox" class="mt-0.5 accent-red-600" />
        <span class="text-sm text-neutral-600 dark:text-neutral-300">
          Connect through an SSH tunnel
          <span class="block text-xs text-neutral-500">
            For a database that only listens on its own machine, or a port your firewall blocks.
          </span>
        </span>
      </label>

      <div
        v-if="draft.useSsh"
        class="mt-3 rounded-xl border border-neutral-200 p-3.5 dark:border-neutral-700"
      >
        <div class="flex gap-3">
          <label class="block flex-1">
            <span :class="LABEL_CLASS">SSH host</span>
            <input
              v-model="draft.sshHost"
              type="text"
              placeholder="203.0.113.10"
              :class="[INPUT_CLASS, 'font-mono']"
            />
          </label>
          <label class="block w-24">
            <span :class="LABEL_CLASS">SSH port</span>
            <input
              v-model.number="draft.sshPort"
              type="number"
              min="1"
              max="65535"
              :class="[INPUT_CLASS, 'font-mono']"
            />
          </label>
        </div>

        <label class="mt-3 block">
          <span :class="LABEL_CLASS">SSH user</span>
          <input v-model="draft.sshUser" type="text" :class="[INPUT_CLASS, 'font-mono']" />
        </label>

        <div class="mt-3">
          <span :class="LABEL_CLASS">SSH authentication</span>
          <div class="mt-1 flex gap-2">
            <button
              type="button"
              class="flex-1 rounded-xl border px-3 py-2 text-sm font-semibold transition"
              :class="
                draft.sshAuth === 'key'
                  ? 'border-red-400 bg-red-50 text-red-700 dark:border-red-500/40 dark:bg-red-500/10 dark:text-red-300'
                  : 'border-neutral-200 text-neutral-600 hover:border-neutral-300 dark:border-neutral-700 dark:text-neutral-300'
              "
              @click="draft.sshAuth = 'key'"
            >
              Private key
            </button>
            <button
              type="button"
              class="flex-1 rounded-xl border px-3 py-2 text-sm font-semibold transition"
              :class="
                draft.sshAuth === 'password'
                  ? 'border-red-400 bg-red-50 text-red-700 dark:border-red-500/40 dark:bg-red-500/10 dark:text-red-300'
                  : 'border-neutral-200 text-neutral-600 hover:border-neutral-300 dark:border-neutral-700 dark:text-neutral-300'
              "
              @click="draft.sshAuth = 'password'"
            >
              Password
            </button>
          </div>
        </div>

        <label v-if="draft.sshAuth === 'key'" class="mt-3 block">
          <span :class="LABEL_CLASS">Private key file</span>
          <div class="mt-1 flex gap-2">
            <input
              v-model="draft.sshKeyPath"
              type="text"
              placeholder="C:\Users\you\.ssh\id_ed25519"
              :class="[INPUT_CLASS, 'mt-0 min-w-0 flex-1 font-mono text-xs']"
            />
            <button
              type="button"
              class="shrink-0 rounded-xl border border-neutral-200 px-3.5 text-sm font-semibold text-neutral-600 transition hover:bg-neutral-50 dark:border-neutral-700 dark:text-neutral-300 dark:hover:bg-neutral-800"
              @click="pickKey"
            >
              Browse
            </button>
          </div>
        </label>

        <label v-else class="mt-3 block">
          <span :class="LABEL_CLASS">SSH password</span>
          <input
            v-model="draft.sshPassword"
            type="password"
            placeholder="••••••••"
            :class="INPUT_CLASS"
          />
        </label>

        <!-- The exact mistake this catches: a password pasted into the key
             box, which otherwise only fails once the tunnel is attempted. -->
        <p
          v-if="keyLooksLikeAPassword"
          class="mt-2 rounded-xl bg-amber-50 px-3 py-2 text-xs text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
        >
          That doesn't look like a file path. This box wants the private <em>key file</em> — if your
          server logs in with a password, switch to
          <strong class="font-semibold">Password</strong> above.
        </p>

        <p v-if="draft.sshAuth === 'key'" class="mt-2.5 text-xs text-neutral-500">
          The key must have no passphrase — Rezure has no console to ask for one on.
        </p>

        <p class="mt-2.5 text-xs text-neutral-500">
          <strong class="font-semibold">Host</strong> and
          <strong class="font-semibold">Port</strong> above are resolved <em>on the SSH server</em>,
          so they're usually <span class="font-mono">127.0.0.1</span> and the database's real port.
        </p>
      </div>

      <div class="mt-4">
        <span :class="LABEL_CLASS">Server engine</span>
        <!-- Not cosmetic: the client binary is chosen from this, and a
             MariaDB client can't authenticate against a MySQL 8 account
             using caching_sha2_password. -->
        <div class="mt-1 flex gap-2">
          <button
            v-for="engine in ENGINES"
            :key="engine"
            type="button"
            class="flex-1 rounded-xl border px-3 py-2 text-sm font-semibold transition"
            :class="
              draft.engine === engine
                ? 'border-red-400 bg-red-50 text-red-700 dark:border-red-500/40 dark:bg-red-500/10 dark:text-red-300'
                : 'border-neutral-200 text-neutral-600 hover:border-neutral-300 dark:border-neutral-700 dark:text-neutral-300'
            "
            @click="draft.engine = engine"
          >
            {{ ENGINE_LABEL[engine] }}
          </button>
        </div>
      </div>

      <div class="mt-4">
        <span :class="LABEL_CLASS">Encryption</span>
        <div class="mt-1 flex gap-2">
          <button
            v-for="mode in TLS_MODES"
            :key="mode.value"
            type="button"
            class="flex-1 rounded-xl border px-3 py-2 text-xs font-semibold transition"
            :class="
              draft.tlsMode === mode.value
                ? 'border-red-400 bg-red-50 text-red-700 dark:border-red-500/40 dark:bg-red-500/10 dark:text-red-300'
                : 'border-neutral-200 text-neutral-600 hover:border-neutral-300 dark:border-neutral-700 dark:text-neutral-300'
            "
            :title="mode.hint"
            @click="draft.tlsMode = mode.value"
          >
            {{ mode.label }}
          </button>
        </div>
      </div>

      <label class="mt-4 flex items-start gap-2.5">
        <input v-model="draft.savePassword" type="checkbox" class="mt-0.5 accent-red-600" />
        <span class="text-sm text-neutral-600 dark:text-neutral-300">
          Save the password in Windows Credential Manager
          <span class="block text-xs text-neutral-500">
            Off means Rezure asks once per session and keeps it in memory only.
          </span>
        </span>
      </label>

      <label class="mt-3 flex items-start gap-2.5">
        <input v-model="draft.readOnly" type="checkbox" class="mt-0.5 accent-red-600" />
        <span class="text-sm text-neutral-600 dark:text-neutral-300">
          Read-only
          <span class="block text-xs text-neutral-500">
            Blocks creating, dropping and importing. Leave this on unless you mean to write to this
            server from Rezure.
          </span>
        </span>
      </label>

      <!-- The test result is the server's own version string, so a green
           state can't be something the UI decided on its own. -->
      <div
        v-if="store.testResult"
        class="mt-5 rounded-xl bg-emerald-50 px-3.5 py-2.5 text-sm text-emerald-800 dark:bg-emerald-500/10 dark:text-emerald-300"
      >
        Connected — server reports
        <span class="font-mono">{{ store.testResult }}</span>
      </div>
      <p v-if="store.testError" class="mt-5 text-sm text-red-600 dark:text-red-400">
        {{ store.testError }}
      </p>
      <p v-if="store.error" class="mt-3 text-sm text-red-600 dark:text-red-400">
        {{ store.error }}
      </p>

      <div class="mt-5 flex items-center justify-between gap-2">
        <button
          type="button"
          class="rounded-full border border-neutral-200 bg-white/70 px-5 py-2.5 text-sm font-semibold text-neutral-700 transition hover:bg-white disabled:opacity-50 dark:border-neutral-700 dark:bg-neutral-900/60 dark:text-neutral-200 dark:hover:bg-neutral-800"
          :disabled="!complete || store.testing"
          @click="store.test(draft)"
        >
          {{ store.testing ? 'Connecting…' : 'Test connection' }}
        </button>

        <div class="flex gap-2">
          <button
            type="button"
            class="rounded-full px-4 py-2.5 text-sm font-semibold text-neutral-500 transition hover:text-neutral-800 dark:hover:text-neutral-200"
            @click="emit('close')"
          >
            Cancel
          </button>
          <button
            type="button"
            class="rounded-full bg-red-600 px-5 py-2.5 text-sm font-semibold text-white shadow-lg shadow-red-500/40 transition hover:bg-red-500 disabled:cursor-not-allowed disabled:opacity-50"
            :disabled="!canSave || store.saving"
            :title="canSave ? '' : 'Test the connection first'"
            @click="submit"
          >
            {{ store.saving ? 'Saving…' : 'Save' }}
          </button>
        </div>
      </div>
    </div>
  </div>
</template>
