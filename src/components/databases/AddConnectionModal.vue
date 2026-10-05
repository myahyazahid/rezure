<script setup lang="ts">
import { computed, onMounted, reactive, watch } from 'vue'
import { open as openFileDialog } from '@tauri-apps/plugin-dialog'
import { useDbConnectionsStore, type ConnectionDraft } from '@/stores/dbConnections'
import { useBinariesStore } from '@/stores/binaries'
import { usePostgresStore } from '@/stores/postgres'
import { ENGINE_LABEL } from '@/types/dbProfile'
import type { DbEngine } from '@/types/dbProfile'
import type { DetectedSqlServer, ServerKind, TlsMode } from '@/types/dbConnection'
import LicenseConsentModal from '@/components/common/LicenseConsentModal.vue'
import { useLicensedInstall } from '@/composables/useLicensedInstall'

const emit = defineEmits<{ close: [] }>()
const store = useDbConnectionsStore()
const binariesStore = useBinariesStore()
const postgresStore = usePostgresStore()
const licensed = useLicensedInstall()

const draft = reactive<ConnectionDraft>({
  name: '',
  host: '',
  port: 3306,
  user: 'root',
  password: '',
  kind: 'mysql',
  engine: 'mysql',
  windowsAuth: false,
  trustServerCertificate: false,
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

const KINDS: { value: ServerKind; label: string }[] = [
  { value: 'mysql', label: 'MySQL / MariaDB' },
  { value: 'sqlserver', label: 'SQL Server' },
  { value: 'postgres', label: 'PostgreSQL' },
]

const DEFAULTS: Record<ServerKind, { port: number; user: string }> = {
  mysql: { port: 3306, user: 'root' },
  sqlserver: { port: 1433, user: 'sa' },
  postgres: { port: 5432, user: 'postgres' },
}

/** Switching the kind moves the port and user along with it — but only while
 *  they still hold the other kind's default, never over something typed. */
watch(
  () => draft.kind,
  (kind, previous) => {
    if (draft.port === DEFAULTS[previous].port) draft.port = DEFAULTS[kind].port
    if (draft.user === DEFAULTS[previous].user) draft.user = DEFAULTS[kind].user
  },
)

/** Looks for SQL Server instances already on this machine the first time the
 *  form shows SQL Server — once per opening, not on every switch of kind. */
let scanned = false
watch(
  () => draft.kind,
  (kind) => {
    if (kind !== 'sqlserver' || scanned) return
    scanned = true
    store.scanSqlServers()
  },
)

/** Fills the form from an instance the scan found. Nothing is saved: the
 *  connection still has to pass a test, like any other. */
function useDetected(found: DetectedSqlServer) {
  draft.name = found.name
  draft.host = found.host
  // Reached by name: an instance on this machine may have a dynamic port, and
  // the ODBC driver finds it without one.
  draft.port = ''
  // The Windows account running Rezure is nearly always a sysadmin on its own
  // machine's instance, and there's no password to ask for.
  draft.windowsAuth = true
  // An installed edition ships a self-signed certificate that ODBC Driver 18
  // refuses; LocalDB talks over a named pipe and has none to complain about.
  draft.trustServerCertificate = found.source === 'service'
  draft.useSsh = false
}

const isSqlServer = computed(() => draft.kind === 'sqlserver')
/** No user or password to ask for: the Windows account signs in. */
const usesWindowsAuth = computed(() => isSqlServer.value && draft.windowsAuth)

/** SQL Server is reached through the Microsoft ODBC Driver; without it a
 *  test can only fail, so the form says so first. */
const odbc = computed(() => binariesStore.binaries.find((b) => b.id === 'msodbcsql') ?? null)
const odbcMissing = computed(
  () => isSqlServer.value && odbc.value !== null && !odbc.value.installed,
)

/** PostgreSQL is reached with `psql` from an installed PostgreSQL build;
 *  without one a test can only fail, so the form says so first. */
const isPostgres = computed(() => draft.kind === 'postgres')
const psqlMissing = computed(() => isPostgres.value && postgresStore.versions.length === 0)

onMounted(() => {
  // Last opening's scan may list something that has been saved since.
  store.detectedSqlServers = []
  if (binariesStore.binaries.length === 0) binariesStore.fetchAll()
  postgresStore.fetchVersions().catch(() => {})
})

/** Installs the newest PostgreSQL — the same as its service card's Install. */
async function installPostgres() {
  await binariesStore.install('postgres').catch(() => {})
  await postgresStore.fetchVersions().catch(() => {})
}

const ENGINES: DbEngine[] = ['mysql', 'mariadb']
const TLS_MODES: { value: TlsMode; label: string; hint: string }[] = [
  { value: 'preferred', label: 'Automatic', hint: 'Use TLS when the server offers it.' },
  { value: 'required', label: 'Required', hint: "Refuse to connect if the server won't." },
  { value: 'disabled', label: 'Off', hint: 'For a private network or an SSH tunnel.' },
]

const complete = computed(() => {
  const base =
    draft.name.trim() !== '' &&
    draft.host.trim() !== '' &&
    (usesWindowsAuth.value || draft.user.trim() !== '') &&
    // A named SQL Server instance may leave the port empty; nothing else can.
    (isSqlServer.value || Number(draft.port) > 0)
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
  'glass-inset mt-1 w-full rounded-xl px-3.5 py-2.5 text-sm text-neutral-900 outline-none transition focus:border-accent-400/70 dark:text-neutral-100'
const LABEL_CLASS = 'block text-xs font-medium text-neutral-500'
</script>

<template>
  <div
    class="glass-scrim fixed inset-0 z-50 flex items-center justify-center p-4"
    @click.self="emit('close')"
  >
    <div class="glass-strong max-h-[88vh] w-full max-w-lg overflow-y-auto rounded-2xl p-6">
      <h2 class="text-lg font-bold text-neutral-900 dark:text-neutral-100">Add connection</h2>
      <p class="mt-1 text-sm text-neutral-500">
        A server Rezure talks to but doesn't run — staging, a VPS, a shared instance. Rezure never
        starts or stops it; it only lists, exports and imports.
      </p>

      <label class="mt-5 block">
        <span :class="LABEL_CLASS">Name</span>
        <input v-model="draft.name" type="text" placeholder="Staging" :class="INPUT_CLASS" />
      </label>

      <div class="mt-4">
        <span :class="LABEL_CLASS">Server type</span>
        <div class="mt-1 flex gap-2">
          <button
            v-for="kind in KINDS"
            :key="kind.value"
            type="button"
            class="flex-1 rounded-xl px-3 py-2 text-sm font-semibold transition"
            :class="
              draft.kind === kind.value
                ? 'glass-selected text-neutral-900 dark:text-neutral-50'
                : 'glass-inset text-neutral-600 hover:bg-white/70 dark:text-neutral-300 dark:hover:bg-white/8'
            "
            @click="draft.kind = kind.value"
          >
            {{ kind.label }}
          </button>
        </div>
      </div>

      <div
        v-if="odbcMissing"
        class="mt-3 rounded-xl bg-amber-100/50 px-3 py-2 text-sm text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
      >
        Rezure talks to SQL Server through the Microsoft ODBC Driver, which isn't installed yet.
        <button
          type="button"
          class="glass-btn mt-2 block rounded-full px-3 py-1 text-xs font-semibold text-amber-900 transition disabled:opacity-50 dark:text-amber-200"
          :disabled="binariesStore.isInstalling('msodbcsql')"
          @click="licensed.request('msodbcsql')"
        >
          {{ binariesStore.isInstalling('msodbcsql') ? 'Installing…' : 'Install ODBC Driver' }}
        </button>
        <p v-if="licensed.error.value" class="mt-1 text-xs text-red-600 dark:text-red-400">
          {{ licensed.error.value }}
        </p>
      </div>

      <div v-if="isSqlServer && store.detectedSqlServers.length > 0" class="mt-4">
        <span :class="LABEL_CLASS">Found on this computer</span>
        <div class="mt-1 flex flex-col gap-1.5">
          <button
            v-for="found in store.detectedSqlServers"
            :key="found.host"
            type="button"
            class="flex items-center justify-between gap-3 rounded-xl px-3.5 py-2 text-left transition"
            :class="
              draft.host === found.host
                ? 'glass-selected text-neutral-900 dark:text-neutral-50'
                : 'glass-inset text-neutral-600 hover:bg-white/70 dark:text-neutral-300 dark:hover:bg-white/8'
            "
            :title="found.running === false ? 'Not running right now' : ''"
            @click="useDetected(found)"
          >
            <span class="min-w-0">
              <span class="block truncate text-sm font-semibold">{{ found.name }}</span>
              <span class="block truncate font-mono text-xs text-neutral-500">
                {{ found.host }}
              </span>
            </span>
            <span
              v-if="found.running !== null"
              class="shrink-0 text-xs font-medium"
              :class="found.running ? 'text-emerald-600 dark:text-emerald-400' : 'text-neutral-500'"
            >
              {{ found.running ? 'Running' : 'Stopped' }}
            </span>
          </button>
        </div>
        <p class="mt-1.5 text-xs text-neutral-500">
          Fills in the form below — you still test the connection before saving it.
        </p>
      </div>

      <div
        v-if="psqlMissing"
        class="mt-3 rounded-xl bg-amber-100/50 px-3 py-2 text-sm text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
      >
        Rezure talks to PostgreSQL with <span class="font-mono">psql</span> from a PostgreSQL build,
        and none is installed yet. Installing one also gives you a local PostgreSQL server.
        <button
          type="button"
          class="glass-btn mt-2 block rounded-full px-3 py-1 text-xs font-semibold text-amber-900 transition disabled:opacity-50 dark:text-amber-200"
          :disabled="binariesStore.isInstalling('postgres')"
          @click="installPostgres"
        >
          {{ binariesStore.isInstalling('postgres') ? 'Installing…' : 'Install PostgreSQL' }}
        </button>
      </div>

      <div class="mt-4 flex gap-3">
        <label class="block flex-1">
          <span :class="LABEL_CLASS">Host</span>
          <input
            v-model="draft.host"
            type="text"
            :placeholder="isSqlServer ? 'db.office.local or host\\SQLEXPRESS' : 'db.example.com'"
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
            :placeholder="isSqlServer ? 'auto' : ''"
            :class="[INPUT_CLASS, 'font-mono']"
          />
        </label>
      </div>
      <p v-if="isSqlServer" class="mt-1.5 text-xs text-neutral-500">
        For a named instance (<span class="font-mono">host\SQLEXPRESS</span>), leave the port empty
        — SQL Server Browser hands it out.
      </p>

      <div v-if="isSqlServer" class="mt-4">
        <span :class="LABEL_CLASS">Sign in with</span>
        <div class="mt-1 flex gap-2">
          <button
            type="button"
            class="flex-1 rounded-xl px-3 py-2 text-sm font-semibold transition"
            :class="
              !draft.windowsAuth
                ? 'glass-selected text-neutral-900 dark:text-neutral-50'
                : 'glass-inset text-neutral-600 hover:bg-white/70 dark:text-neutral-300 dark:hover:bg-white/8'
            "
            @click="draft.windowsAuth = false"
          >
            SQL Server login
          </button>
          <button
            type="button"
            class="flex-1 rounded-xl px-3 py-2 text-sm font-semibold transition"
            :class="
              draft.windowsAuth
                ? 'glass-selected text-neutral-900 dark:text-neutral-50'
                : 'glass-inset text-neutral-600 hover:bg-white/70 dark:text-neutral-300 dark:hover:bg-white/8'
            "
            @click="draft.windowsAuth = true"
          >
            Windows account
          </button>
        </div>
        <p v-if="draft.windowsAuth" class="mt-1.5 text-xs text-neutral-500">
          Signs in as the Windows user Rezure runs as — no password is stored anywhere. Works for
          servers on this machine and on your office domain.
        </p>
      </div>

      <div v-if="!usesWindowsAuth" class="mt-4 flex gap-3">
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
        <input v-model="draft.useSsh" type="checkbox" class="mt-0.5 accent-accent-600" />
        <span class="text-sm text-neutral-600 dark:text-neutral-300">
          Connect through an SSH tunnel
          <span class="block text-xs text-neutral-500">
            For a database that only listens on its own machine, or a port your firewall blocks.
          </span>
        </span>
      </label>

      <div v-if="draft.useSsh" class="glass-divider mt-3 rounded-xl border p-3.5">
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
              class="flex-1 rounded-xl px-3 py-2 text-sm font-semibold transition"
              :class="
                draft.sshAuth === 'key'
                  ? 'glass-selected text-neutral-900 dark:text-neutral-50'
                  : 'glass-inset text-neutral-600 hover:bg-white/70 dark:text-neutral-300 dark:hover:bg-white/8'
              "
              @click="draft.sshAuth = 'key'"
            >
              Private key
            </button>
            <button
              type="button"
              class="flex-1 rounded-xl px-3 py-2 text-sm font-semibold transition"
              :class="
                draft.sshAuth === 'password'
                  ? 'glass-selected text-neutral-900 dark:text-neutral-50'
                  : 'glass-inset text-neutral-600 hover:bg-white/70 dark:text-neutral-300 dark:hover:bg-white/8'
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
              class="glass-btn shrink-0 rounded-xl px-3.5 text-sm font-semibold text-neutral-600 transition dark:text-neutral-300"
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
          class="mt-2 rounded-xl bg-amber-100/50 px-3 py-2 text-xs text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
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

      <div v-if="draft.kind === 'mysql'" class="mt-4">
        <span :class="LABEL_CLASS">Server engine</span>
        <!-- Not cosmetic: the client binary is chosen from this, and a
             MariaDB client can't authenticate against a MySQL 8 account
             using caching_sha2_password. -->
        <div class="mt-1 flex gap-2">
          <button
            v-for="engine in ENGINES"
            :key="engine"
            type="button"
            class="flex-1 rounded-xl px-3 py-2 text-sm font-semibold transition"
            :class="
              draft.engine === engine
                ? 'glass-selected text-neutral-900 dark:text-neutral-50'
                : 'glass-inset text-neutral-600 hover:bg-white/70 dark:text-neutral-300 dark:hover:bg-white/8'
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
            class="flex-1 rounded-xl px-3 py-2 text-xs font-semibold transition"
            :class="
              draft.tlsMode === mode.value
                ? 'glass-selected text-neutral-900 dark:text-neutral-50'
                : 'glass-inset text-neutral-600 hover:bg-white/70 dark:text-neutral-300 dark:hover:bg-white/8'
            "
            :title="mode.hint"
            @click="draft.tlsMode = mode.value"
          >
            {{ mode.label }}
          </button>
        </div>
      </div>

      <!-- ODBC Driver 18 refuses a self-signed certificate, which most
           development and office servers have. Off unless asked for: it
           trades away the check that this is the server you meant. -->
      <label v-if="isSqlServer" class="mt-4 flex items-start gap-2.5">
        <input
          v-model="draft.trustServerCertificate"
          type="checkbox"
          class="mt-0.5 accent-accent-600"
        />
        <span class="text-sm text-neutral-600 dark:text-neutral-300">
          Trust server certificate
          <span class="block text-xs text-neutral-500">
            For a server with a self-signed certificate. Only turn this on for a server you trust —
            it skips checking who you're connected to.
          </span>
        </span>
      </label>

      <label v-if="!usesWindowsAuth" class="mt-4 flex items-start gap-2.5">
        <input v-model="draft.savePassword" type="checkbox" class="mt-0.5 accent-accent-600" />
        <span class="text-sm text-neutral-600 dark:text-neutral-300">
          Save the password in Windows Credential Manager
          <span class="block text-xs text-neutral-500">
            Off means Rezure asks once per session and keeps it in memory only.
          </span>
        </span>
      </label>

      <label class="mt-3 flex items-start gap-2.5">
        <input v-model="draft.readOnly" type="checkbox" class="mt-0.5 accent-accent-600" />
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
        class="mt-5 rounded-xl bg-emerald-100/50 px-3.5 py-2.5 text-sm text-emerald-800 dark:bg-emerald-500/10 dark:text-emerald-300"
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
          class="glass-btn rounded-full px-4 py-2 text-sm font-semibold text-neutral-700 transition disabled:opacity-50 dark:text-neutral-200"
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
            class="glass-accent rounded-full px-4 py-2 text-sm font-semibold transition disabled:cursor-not-allowed disabled:opacity-50"
            :disabled="!canSave || store.saving"
            :title="canSave ? '' : 'Test the connection first'"
            @click="submit"
          >
            {{ store.saving ? 'Saving…' : 'Save' }}
          </button>
        </div>
      </div>
    </div>

    <LicenseConsentModal
      v-if="licensed.pending.value"
      :pkg="licensed.pending.value"
      @confirm="licensed.confirm"
      @close="licensed.cancel"
    />
  </div>
</template>
