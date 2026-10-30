<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from 'vue'
import { open as openFileDialog } from '@tauri-apps/plugin-dialog'
import { useDbConnectionsStore, type ConnectionDraft } from '@/stores/dbConnections'
import { useBinariesStore } from '@/stores/binaries'
import { usePostgresStore } from '@/stores/postgres'
import { ENGINE_LABEL } from '@/types/dbProfile'
import type { DbEngine } from '@/types/dbProfile'
import { TLS_LABEL } from '@/types/dbConnection'
import type { DetectedSqlServer, ServerKind, TlsMode } from '@/types/dbConnection'
import LicenseConsentModal from '@/components/common/LicenseConsentModal.vue'
import BasePill from '@/components/common/BasePill.vue'
import TechIcon from '@/components/common/TechIcon.vue'
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

/** Purely presentational, so neither belongs in the draft. */
const showPassword = ref(false)
/** Encryption and the password-storage choice: settled once and rarely
 *  revisited, so they sit behind a disclosure with a summary. */
const showMore = ref(false)

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
  { value: 'preferred', label: 'Automatic', hint: 'Uses TLS when the server offers it.' },
  { value: 'required', label: 'Required', hint: "Refuses to connect if the server won't use TLS." },
  {
    value: 'disabled',
    label: 'Off',
    hint: 'For a private network, or when an SSH tunnel already encrypts the link.',
  },
]
const tlsHint = computed(() => TLS_MODES.find((m) => m.value === draft.tlsMode)?.hint ?? '')

/** The brand mark for what this connection will talk to. A MySQL-family
 *  server shows the engine actually picked. */
const kindIcon = computed(() => (draft.kind === 'mysql' ? draft.engine : draft.kind))

/** The same endpoint line the connection switcher shows for a saved
 *  connection (`describeConnection` in DbTargetSwitcher), so the preview
 *  reads exactly like the row it turns into. */
const previewEndpoint = computed(() => {
  const host = draft.host.trim() || 'host'
  const server = draft.port ? `${host}:${draft.port}` : host
  const endpoint = isSqlServer.value
    ? `${usesWindowsAuth.value ? 'Windows sign-in' : draft.user.trim() || 'user'} · ${server}`
    : `${draft.user.trim() || 'user'}@${server}`
  const sshHost = draft.sshHost.trim()
  return draft.useSsh && sshHost ? `${endpoint} via ${sshHost}` : endpoint
})

/** What "More options" holds while it is closed, so a changed setting is
 *  never hidden. */
const moreSummary = computed(() => {
  const parts: string[] = [TLS_LABEL[draft.tlsMode]]
  if (isSqlServer.value && draft.trustServerCertificate) parts.push('any certificate')
  if (!usesWindowsAuth.value)
    parts.push(draft.savePassword ? 'password saved' : 'asks each session')
  return parts.join(' · ')
})

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

const LABEL_CLASS = 'mb-1.5 block text-xs font-medium text-neutral-500'
const OVERLINE_CLASS = 'text-[11px] font-bold tracking-wider text-neutral-500 uppercase'
/** A field is a well holding a leading icon, the input, and sometimes a
 *  suffix (the port, a reveal button) — one focus ring around all of it. */
const FIELD_CLASS =
  'glass-inset flex h-11 items-center gap-2.5 rounded-xl px-3 transition focus-within:border-accent-400/70 focus-within:ring-4 focus-within:ring-accent-500/15'
const INPUT_CLASS =
  'h-full min-w-0 flex-1 bg-transparent text-sm text-neutral-900 outline-none placeholder:text-neutral-400 dark:text-neutral-100 dark:placeholder:text-neutral-500'
const FIELD_ICON_CLASS = 'h-4 w-4 shrink-0 text-neutral-400'
/** Leading icon of a switch row: plain and neutral, so the switch is what
 *  carries the state. */
const ROW_ICON_CLASS = 'h-5 w-5 shrink-0 text-neutral-400 dark:text-neutral-500'
const SEGMENTS_CLASS = 'glass-inset flex gap-0.5 rounded-xl p-[3px]'
const SEGMENT_CLASS =
  'flex h-9 flex-1 items-center justify-center gap-2 rounded-[9px] border px-2 text-[13px] font-semibold whitespace-nowrap transition'
/** Brand marks are drawn in their own colours, several of which (MariaDB's
 *  navy, MySQL's blue) vanish on dark glass — so they sit on a light tile in
 *  both themes, the way vendor logos usually do. */
const LOGO_TILE_CLASS =
  'grid shrink-0 place-items-center rounded-xl bg-white shadow-sm ring-1 ring-neutral-900/5'
const NOTICE_CLASS =
  'rounded-xl bg-amber-100/50 px-3.5 py-2.5 text-sm text-amber-900 dark:bg-amber-500/10 dark:text-amber-200'
const SWITCH_CLASS = 'relative h-6 w-11 shrink-0 rounded-full transition'
const SWITCH_KNOB_CLASS = 'absolute top-0.5 h-5 w-5 rounded-full bg-white shadow transition'

function segmentClass(on: boolean) {
  return on
    ? 'glass-selected text-neutral-900 dark:text-neutral-50'
    : 'border-transparent text-neutral-500 hover:text-neutral-800 dark:text-neutral-400 dark:hover:text-neutral-100'
}

function switchClass(on: boolean) {
  return on ? 'bg-accent-600' : 'bg-neutral-900/15 dark:bg-white/15'
}
</script>

<template>
  <div
    class="glass-scrim fixed inset-0 z-50 flex items-center justify-center p-6"
    @click.self="emit('close')"
  >
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="add-connection-title"
      class="dialog glass-strong grid h-[min(700px,90vh)] w-full max-w-[940px] grid-cols-[300px_minmax(0,1fr)] overflow-hidden rounded-3xl"
    >
      <!-- What is being connected to, and what it will look like once
           saved. The form on the right only ever holds the details. -->
      <aside
        class="glass-divider flex min-h-0 flex-col gap-6 overflow-y-auto border-r bg-neutral-900/[0.025] p-6 dark:bg-black/20"
      >
        <div>
          <h2
            id="add-connection-title"
            class="text-xl font-bold text-neutral-900 dark:text-neutral-100"
          >
            New connection
          </h2>
          <p class="mt-1 text-[13px] leading-relaxed text-neutral-500">
            A server Rezure talks to but doesn't run. It only lists, exports and imports.
          </p>
        </div>

        <div>
          <p :class="[OVERLINE_CLASS, 'mb-2']">Server type</p>
          <div class="flex flex-col gap-1.5">
            <button
              v-for="kind in KINDS"
              :key="kind.value"
              type="button"
              class="flex w-full items-center gap-3 rounded-2xl border p-2.5 text-left transition"
              :class="
                draft.kind === kind.value
                  ? 'glass-selected'
                  : 'border-transparent hover:bg-white/60 dark:hover:bg-white/5'
              "
              :aria-pressed="draft.kind === kind.value"
              @click="draft.kind = kind.value"
            >
              <span :class="[LOGO_TILE_CLASS, 'h-10 w-10']">
                <TechIcon :id="kind.value === 'mysql' ? draft.engine : kind.value" :size="24" />
              </span>
              <span class="min-w-0 flex-1">
                <span
                  class="block truncate text-sm font-semibold text-neutral-900 dark:text-neutral-100"
                >
                  {{ kind.label }}
                </span>
                <span class="block font-mono text-[11px] text-neutral-500">
                  port {{ DEFAULTS[kind.value].port }} · {{ DEFAULTS[kind.value].user }}
                </span>
              </span>
              <span
                class="grid h-5 w-5 shrink-0 place-items-center rounded-full border-2 transition"
                :class="
                  draft.kind === kind.value
                    ? 'border-accent-500 bg-accent-500 text-white'
                    : 'border-neutral-900/15 text-transparent dark:border-white/15'
                "
              >
                <svg
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="4"
                  class="h-2.5 w-2.5"
                >
                  <path stroke-linecap="round" stroke-linejoin="round" d="M5 12l5 5L20 7" />
                </svg>
              </span>
            </button>
          </div>

          <div v-if="draft.kind === 'mysql'" class="reveal mt-3">
            <span :class="LABEL_CLASS">Server engine</span>
            <!-- Not cosmetic: the client binary is chosen from this, and a
                 MariaDB client can't authenticate against a MySQL 8 account
                 using caching_sha2_password. -->
            <div :class="SEGMENTS_CLASS">
              <button
                v-for="engine in ENGINES"
                :key="engine"
                type="button"
                :class="[SEGMENT_CLASS, segmentClass(draft.engine === engine)]"
                @click="draft.engine = engine"
              >
                {{ ENGINE_LABEL[engine] }}
              </button>
            </div>
          </div>
        </div>

        <!-- The row this becomes in the connection switcher, live: the
             quickest way to spot a wrong host or user before testing. -->
        <div class="mt-auto">
          <p :class="[OVERLINE_CLASS, 'mb-2']">How it will look in your list</p>
          <div class="glass rounded-2xl p-3.5">
            <div class="flex items-center gap-3">
              <span :class="[LOGO_TILE_CLASS, 'h-10 w-10']">
                <TechIcon :id="kindIcon" :size="24" />
              </span>
              <span class="min-w-0 flex-1">
                <span
                  class="block truncate text-sm font-bold text-neutral-900 dark:text-neutral-100"
                >
                  {{ draft.name.trim() || 'Untitled connection' }}
                </span>
                <span class="block truncate font-mono text-[11px] text-neutral-500">
                  {{ previewEndpoint }}
                </span>
              </span>
            </div>
            <div class="mt-3 flex flex-wrap gap-1.5">
              <span
                v-if="store.testResult"
                class="inline-flex items-center gap-1 rounded-full bg-emerald-100/60 px-2 py-0.5 text-xs font-semibold text-emerald-700 dark:bg-emerald-500/10 dark:text-emerald-300"
              >
                <span class="h-1.5 w-1.5 rounded-full bg-emerald-500" />
                Online
              </span>
              <span
                v-if="draft.readOnly"
                class="rounded-full bg-emerald-100/60 px-2 py-0.5 text-xs font-semibold text-emerald-700 dark:bg-emerald-500/10 dark:text-emerald-300"
              >
                Read-only
              </span>
              <span
                v-else
                class="rounded-full bg-amber-100/60 px-2 py-0.5 text-xs font-semibold text-amber-700 dark:bg-amber-500/10 dark:text-amber-300"
              >
                Read &amp; write
              </span>
              <BasePill v-if="draft.useSsh">SSH</BasePill>
              <BasePill>{{ TLS_LABEL[draft.tlsMode] }}</BasePill>
            </div>
          </div>
        </div>
      </aside>

      <section class="flex min-h-0 min-w-0 flex-col">
        <div class="flex justify-end px-4 pt-4">
          <button
            type="button"
            class="glass-ghost grid h-9 w-9 place-items-center rounded-xl"
            aria-label="Close"
            @click="emit('close')"
          >
            <svg
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
              class="h-[18px] w-[18px]"
            >
              <path stroke-linecap="round" d="M6 6l12 12M18 6L6 18" />
            </svg>
          </button>
        </div>

        <div class="min-h-0 flex-1 space-y-5 overflow-y-auto px-8 pb-6">
          <div v-if="odbcMissing" :class="NOTICE_CLASS">
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

          <div v-if="psqlMissing" :class="NOTICE_CLASS">
            Rezure talks to PostgreSQL with <span class="font-mono">psql</span> from a PostgreSQL
            build, and none is installed yet. Installing one also gives you a local PostgreSQL
            server.
            <button
              type="button"
              class="glass-btn mt-2 block rounded-full px-3 py-1 text-xs font-semibold text-amber-900 transition disabled:opacity-50 dark:text-amber-200"
              :disabled="binariesStore.isInstalling('postgres')"
              @click="installPostgres"
            >
              {{ binariesStore.isInstalling('postgres') ? 'Installing…' : 'Install PostgreSQL' }}
            </button>
          </div>

          <div v-if="isSqlServer && store.detectedSqlServers.length > 0">
            <span :class="LABEL_CLASS">Found on this computer</span>
            <div class="flex flex-col gap-1.5">
              <button
                v-for="found in store.detectedSqlServers"
                :key="found.host"
                type="button"
                class="flex items-center justify-between gap-3 rounded-xl border px-3.5 py-2.5 text-left transition"
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
                  :class="
                    found.running ? 'text-emerald-600 dark:text-emerald-400' : 'text-neutral-500'
                  "
                >
                  {{ found.running ? 'Running' : 'Stopped' }}
                </span>
              </button>
            </div>
            <p class="mt-1.5 text-xs text-neutral-500">
              Fills in the form below - you still test the connection before saving it.
            </p>
          </div>

          <div>
            <label for="conn-name" :class="LABEL_CLASS">Name</label>
            <div :class="FIELD_CLASS">
              <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                :class="FIELD_ICON_CLASS"
              >
                <path
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  d="M20.6 13.4l-7.2 7.2a2 2 0 01-2.8 0L3 13V3h10l7.6 7.6a2 2 0 010 2.8zM7.5 7.5h.01"
                />
              </svg>
              <input
                id="conn-name"
                v-model="draft.name"
                type="text"
                placeholder="Staging"
                :class="INPUT_CLASS"
              />
            </div>
          </div>

          <div>
            <label for="conn-host" :class="LABEL_CLASS">
              {{ draft.useSsh ? 'Database address, as seen from the SSH server' : 'Address' }}
            </label>
            <div :class="FIELD_CLASS">
              <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                :class="FIELD_ICON_CLASS"
              >
                <circle cx="12" cy="12" r="9" />
                <path d="M3 12h18M12 3a14 14 0 010 18M12 3a14 14 0 000 18" />
              </svg>
              <input
                id="conn-host"
                v-model="draft.host"
                type="text"
                :placeholder="
                  isSqlServer ? 'db.office.local or host\\SQLEXPRESS' : 'db.example.com'
                "
                :class="[INPUT_CLASS, 'font-mono']"
              />
              <span
                class="flex h-6 shrink-0 items-center border-l border-neutral-900/10 pl-2.5 font-mono text-sm text-neutral-400 dark:border-white/10"
              >
                <span aria-hidden="true">:</span>
                <input
                  v-model.number="draft.port"
                  type="number"
                  min="1"
                  max="65535"
                  aria-label="Port"
                  :placeholder="isSqlServer ? 'auto' : ''"
                  class="no-spin w-14 bg-transparent pl-0.5 text-neutral-900 outline-none placeholder:text-neutral-400 dark:text-neutral-100"
                />
              </span>
            </div>
            <p v-if="isSqlServer" class="mt-1.5 text-xs text-neutral-500">
              For a named instance (<span class="font-mono">host\SQLEXPRESS</span>), leave the port
              empty - SQL Server Browser hands it out.
            </p>
            <p v-else-if="draft.useSsh" class="mt-1.5 text-xs text-neutral-500">
              Resolved on the SSH server, so usually <span class="font-mono">127.0.0.1</span> and
              the database's own port.
            </p>
          </div>

          <div v-if="isSqlServer">
            <span :class="LABEL_CLASS">Sign in with</span>
            <div :class="SEGMENTS_CLASS">
              <button
                type="button"
                :class="[SEGMENT_CLASS, segmentClass(!draft.windowsAuth)]"
                @click="draft.windowsAuth = false"
              >
                SQL Server login
              </button>
              <button
                type="button"
                :class="[SEGMENT_CLASS, segmentClass(draft.windowsAuth)]"
                @click="draft.windowsAuth = true"
              >
                Windows account
              </button>
            </div>
            <p v-if="draft.windowsAuth" class="mt-1.5 text-xs text-neutral-500">
              Signs in as the Windows user Rezure runs as - no password is stored anywhere. Works
              for servers on this machine and on your office domain.
            </p>
          </div>

          <div v-if="!usesWindowsAuth" class="grid grid-cols-2 gap-3">
            <div>
              <label for="conn-user" :class="LABEL_CLASS">User</label>
              <div :class="FIELD_CLASS">
                <svg
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="2"
                  :class="FIELD_ICON_CLASS"
                >
                  <circle cx="12" cy="8" r="4" />
                  <path stroke-linecap="round" d="M4 21a8 8 0 0116 0" />
                </svg>
                <input
                  id="conn-user"
                  v-model="draft.user"
                  type="text"
                  :class="[INPUT_CLASS, 'font-mono']"
                />
              </div>
            </div>
            <div>
              <label for="conn-password" :class="LABEL_CLASS">Password</label>
              <div :class="FIELD_CLASS">
                <svg
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="2"
                  :class="FIELD_ICON_CLASS"
                >
                  <rect x="4" y="11" width="16" height="10" rx="2" />
                  <path d="M8 11V7a4 4 0 018 0v4" />
                </svg>
                <input
                  id="conn-password"
                  v-model="draft.password"
                  :type="showPassword ? 'text' : 'password'"
                  placeholder="••••••••"
                  :class="INPUT_CLASS"
                />
                <button
                  type="button"
                  class="glass-ghost -mr-1 grid h-7 w-7 shrink-0 place-items-center rounded-lg"
                  :aria-label="showPassword ? 'Hide password' : 'Show password'"
                  :aria-pressed="showPassword"
                  @click="showPassword = !showPassword"
                >
                  <svg
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="2"
                    class="h-4 w-4"
                  >
                    <path d="M2 12s3.6-7 10-7 10 7 10 7-3.6 7-10 7S2 12 2 12z" />
                    <circle cx="12" cy="12" r="3" />
                    <path v-if="showPassword" stroke-linecap="round" d="M4 4l16 16" />
                  </svg>
                </button>
              </div>
            </div>
          </div>

          <div class="glass-divider divide-y divide-neutral-900/8 border-y dark:divide-white/8">
            <div>
              <div class="flex items-center gap-3.5 py-3.5">
                <!-- A route through one stop: the hop the tunnel adds. -->
                <svg
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  stroke-width="1.75"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  :class="ROW_ICON_CLASS"
                >
                  <circle cx="6" cy="19" r="3" />
                  <path d="M9 19h8.5a3.5 3.5 0 0 0 0-7h-11a3.5 3.5 0 0 1 0-7H15" />
                  <circle cx="18" cy="5" r="3" />
                </svg>
                <span class="min-w-0 flex-1">
                  <span class="block text-sm font-semibold text-neutral-900 dark:text-neutral-100">
                    Connect through SSH
                  </span>
                  <span class="block text-xs text-neutral-500">
                    For a database that only listens on its own machine, or a port your firewall
                    blocks.
                  </span>
                </span>
                <button
                  type="button"
                  role="switch"
                  :aria-checked="draft.useSsh"
                  aria-label="Connect through SSH"
                  :class="[SWITCH_CLASS, switchClass(draft.useSsh)]"
                  @click="draft.useSsh = !draft.useSsh"
                >
                  <span :class="[SWITCH_KNOB_CLASS, draft.useSsh ? 'left-5' : 'left-0.5']" />
                </button>
              </div>

              <div v-if="draft.useSsh" class="reveal space-y-3 pb-4 pl-8.5">
                <div class="flex gap-3">
                  <div class="min-w-0 flex-1">
                    <label for="conn-ssh-host" :class="LABEL_CLASS">SSH host</label>
                    <div :class="FIELD_CLASS">
                      <input
                        id="conn-ssh-host"
                        v-model="draft.sshHost"
                        type="text"
                        placeholder="203.0.113.10"
                        :class="[INPUT_CLASS, 'font-mono']"
                      />
                      <span
                        class="flex h-6 shrink-0 items-center border-l border-neutral-900/10 pl-2.5 font-mono text-sm text-neutral-400 dark:border-white/10"
                      >
                        <span aria-hidden="true">:</span>
                        <input
                          v-model.number="draft.sshPort"
                          type="number"
                          min="1"
                          max="65535"
                          aria-label="SSH port"
                          class="no-spin w-10 bg-transparent pl-0.5 text-neutral-900 outline-none dark:text-neutral-100"
                        />
                      </span>
                    </div>
                  </div>
                  <div class="w-36 shrink-0">
                    <label for="conn-ssh-user" :class="LABEL_CLASS">SSH user</label>
                    <div :class="FIELD_CLASS">
                      <input
                        id="conn-ssh-user"
                        v-model="draft.sshUser"
                        type="text"
                        :class="[INPUT_CLASS, 'font-mono']"
                      />
                    </div>
                  </div>
                </div>

                <div :class="SEGMENTS_CLASS">
                  <button
                    type="button"
                    :class="[SEGMENT_CLASS, segmentClass(draft.sshAuth === 'key')]"
                    @click="draft.sshAuth = 'key'"
                  >
                    Private key
                  </button>
                  <button
                    type="button"
                    :class="[SEGMENT_CLASS, segmentClass(draft.sshAuth === 'password')]"
                    @click="draft.sshAuth = 'password'"
                  >
                    Password
                  </button>
                </div>

                <div v-if="draft.sshAuth === 'key'">
                  <label for="conn-ssh-key" :class="LABEL_CLASS">Private key file</label>
                  <div :class="FIELD_CLASS">
                    <svg
                      viewBox="0 0 24 24"
                      fill="none"
                      stroke="currentColor"
                      stroke-width="2"
                      :class="FIELD_ICON_CLASS"
                    >
                      <circle cx="8" cy="15" r="4" />
                      <path stroke-linecap="round" d="M10.8 12.2L20 3M16 7l3 3M14 9l2 2" />
                    </svg>
                    <input
                      id="conn-ssh-key"
                      v-model="draft.sshKeyPath"
                      type="text"
                      placeholder="C:\Users\you\.ssh\id_ed25519"
                      :class="[INPUT_CLASS, 'font-mono text-xs']"
                    />
                    <button
                      type="button"
                      class="glass-btn -mr-1 shrink-0 rounded-lg px-3 py-1 text-xs font-semibold text-neutral-700 transition dark:text-neutral-200"
                      @click="pickKey"
                    >
                      Browse
                    </button>
                  </div>
                  <p class="mt-1.5 text-xs text-neutral-500">
                    The key must have no passphrase - Rezure has no console to ask for one on.
                  </p>
                </div>

                <div v-else>
                  <label for="conn-ssh-password" :class="LABEL_CLASS">SSH password</label>
                  <div :class="FIELD_CLASS">
                    <input
                      id="conn-ssh-password"
                      v-model="draft.sshPassword"
                      type="password"
                      placeholder="••••••••"
                      :class="INPUT_CLASS"
                    />
                  </div>
                </div>

                <!-- The exact mistake this catches: a password pasted into the key
                     box, which otherwise only fails once the tunnel is attempted. -->
                <p v-if="keyLooksLikeAPassword" :class="[NOTICE_CLASS, 'text-xs']">
                  That doesn't look like a file path. This box wants the private
                  <em>key file</em> - if your server logs in with a password, switch to
                  <strong class="font-semibold">Password</strong> above.
                </p>
              </div>
            </div>

            <div class="flex items-center gap-3.5 py-3.5">
              <!-- A struck-out pencil: nothing gets written. -->
              <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="1.75"
                stroke-linecap="round"
                stroke-linejoin="round"
                :class="ROW_ICON_CLASS"
              >
                <path
                  d="m10 10-6.157 6.162a2 2 0 0 0-.5.833l-1.322 4.36a.5.5 0 0 0 .622.624l4.358-1.323a2 2 0 0 0 .83-.5L14 13.982"
                />
                <path d="m12.829 7.172 4.359-4.346a1 1 0 1 1 3.986 3.986l-4.353 4.353" />
                <path d="m2 2 20 20" />
              </svg>
              <span class="min-w-0 flex-1">
                <span class="block text-sm font-semibold text-neutral-900 dark:text-neutral-100">
                  Read-only
                </span>
                <span class="block text-xs text-neutral-500">
                  Blocks creating, dropping and importing. Leave this on unless you mean to write
                  here.
                </span>
              </span>
              <button
                type="button"
                role="switch"
                :aria-checked="draft.readOnly"
                aria-label="Read-only"
                :class="[SWITCH_CLASS, switchClass(draft.readOnly)]"
                @click="draft.readOnly = !draft.readOnly"
              >
                <span :class="[SWITCH_KNOB_CLASS, draft.readOnly ? 'left-5' : 'left-0.5']" />
              </button>
            </div>
          </div>

          <div class="-mt-2">
            <button
              type="button"
              class="flex w-full items-center gap-2 py-2 text-left text-sm font-semibold text-neutral-900 dark:text-neutral-100"
              :aria-expanded="showMore"
              @click="showMore = !showMore"
            >
              <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2.2"
                class="h-4 w-4 shrink-0 text-neutral-400 transition-transform"
                :class="showMore ? 'rotate-90' : ''"
              >
                <path stroke-linecap="round" stroke-linejoin="round" d="M9 6l6 6-6 6" />
              </svg>
              More options
              <span class="ml-auto truncate pl-3 text-xs font-normal text-neutral-500">
                {{ moreSummary }}
              </span>
            </button>

            <div v-if="showMore" class="reveal space-y-4 pt-2 pl-6">
              <div>
                <span :class="LABEL_CLASS">Encryption</span>
                <div :class="SEGMENTS_CLASS">
                  <button
                    v-for="mode in TLS_MODES"
                    :key="mode.value"
                    type="button"
                    :class="[SEGMENT_CLASS, segmentClass(draft.tlsMode === mode.value)]"
                    @click="draft.tlsMode = mode.value"
                  >
                    {{ mode.label }}
                  </button>
                </div>
                <p class="mt-1.5 text-xs text-neutral-500">{{ tlsHint }}</p>
              </div>

              <!-- ODBC Driver 18 refuses a self-signed certificate, which most
                   development and office servers have. Off unless asked for: it
                   trades away the check that this is the server you meant. -->
              <div v-if="isSqlServer" class="flex items-center gap-3.5">
                <span class="min-w-0 flex-1">
                  <span class="block text-sm font-semibold text-neutral-900 dark:text-neutral-100">
                    Trust server certificate
                  </span>
                  <span class="block text-xs text-neutral-500">
                    For a self-signed certificate. Only for a server you trust - it skips checking
                    who you're connected to.
                  </span>
                </span>
                <button
                  type="button"
                  role="switch"
                  :aria-checked="draft.trustServerCertificate"
                  aria-label="Trust server certificate"
                  :class="[SWITCH_CLASS, switchClass(draft.trustServerCertificate)]"
                  @click="draft.trustServerCertificate = !draft.trustServerCertificate"
                >
                  <span
                    :class="[
                      SWITCH_KNOB_CLASS,
                      draft.trustServerCertificate ? 'left-5' : 'left-0.5',
                    ]"
                  />
                </button>
              </div>

              <div v-if="!usesWindowsAuth" class="flex items-center gap-3.5">
                <span class="min-w-0 flex-1">
                  <span class="block text-sm font-semibold text-neutral-900 dark:text-neutral-100">
                    Remember password
                  </span>
                  <span class="block text-xs text-neutral-500">
                    Saved in Windows Credential Manager. Off asks once per session and keeps it in
                    memory only.
                  </span>
                </span>
                <button
                  type="button"
                  role="switch"
                  :aria-checked="draft.savePassword"
                  aria-label="Remember password"
                  :class="[SWITCH_CLASS, switchClass(draft.savePassword)]"
                  @click="draft.savePassword = !draft.savePassword"
                >
                  <span :class="[SWITCH_KNOB_CLASS, draft.savePassword ? 'left-5' : 'left-0.5']" />
                </button>
              </div>
            </div>
          </div>
        </div>

        <footer class="glass-divider flex items-center gap-2.5 border-t px-6 py-4">
          <!-- The test result is the server's own version string, so a green
               state can't be something the UI decided on its own. -->
          <div class="flex min-w-0 flex-1 items-start gap-2 text-[13px]">
            <template v-if="store.testing">
              <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2.5"
                class="mt-0.5 h-4 w-4 shrink-0 animate-spin text-accent-500"
              >
                <path stroke-linecap="round" d="M21 12a9 9 0 11-6.2-8.6" />
              </svg>
              <span class="text-neutral-600 dark:text-neutral-300">
                {{ draft.useSsh ? 'Opening the SSH tunnel…' : 'Connecting…' }}
              </span>
            </template>
            <template v-else-if="store.testResult">
              <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2.5"
                class="mt-0.5 h-4 w-4 shrink-0 text-emerald-600 dark:text-emerald-400"
              >
                <circle cx="12" cy="12" r="9" />
                <path stroke-linecap="round" stroke-linejoin="round" d="M8 12l3 3 5-6" />
              </svg>
              <span class="min-w-0 truncate text-emerald-700 dark:text-emerald-300">
                Connected · <span class="font-mono">{{ store.testResult }}</span>
              </span>
            </template>
            <template v-else-if="store.testError || store.error">
              <svg
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2.5"
                class="mt-0.5 h-4 w-4 shrink-0 text-red-600 dark:text-red-400"
              >
                <circle cx="12" cy="12" r="9" />
                <path stroke-linecap="round" d="M12 8v4M12 16h.01" />
              </svg>
              <span class="max-h-16 min-w-0 overflow-y-auto text-red-600 dark:text-red-400">
                {{ store.testError ?? store.error }}
              </span>
            </template>
            <span v-else class="text-neutral-500">Test the connection to unlock Save.</span>
          </div>

          <button
            type="button"
            class="shrink-0 rounded-xl px-3 py-2.5 text-sm font-semibold text-neutral-500 transition hover:text-neutral-800 dark:hover:text-neutral-200"
            @click="emit('close')"
          >
            Cancel
          </button>
          <button
            type="button"
            class="glass-btn flex shrink-0 items-center gap-2 rounded-xl px-4 py-2.5 text-sm font-semibold text-neutral-700 transition disabled:opacity-50 dark:text-neutral-200"
            :disabled="!complete || store.testing"
            @click="store.test(draft)"
          >
            <svg
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
              class="h-4 w-4"
            >
              <path stroke-linejoin="round" d="M13 2L4 14h7l-1 8 9-12h-7l1-8z" />
            </svg>
            {{ store.testing ? 'Testing…' : store.testResult ? 'Test again' : 'Test' }}
          </button>
          <button
            type="button"
            class="glass-accent shrink-0 rounded-xl px-5 py-2.5 text-sm font-semibold transition disabled:cursor-not-allowed disabled:opacity-50"
            :disabled="!canSave || store.saving"
            :title="canSave ? '' : 'Test the connection first'"
            @click="submit"
          >
            {{ store.saving ? 'Saving…' : 'Save connection' }}
          </button>
        </footer>
      </section>
    </div>

    <LicenseConsentModal
      v-if="licensed.pending.value"
      :pkg="licensed.pending.value"
      @confirm="licensed.confirm"
      @close="licensed.cancel"
    />
  </div>
</template>

<style scoped>
/* The app's Reduce motion setting (`.reduce-motion` in main.css) already
   stops these. */
.dialog {
  animation: dialog-in 0.22s cubic-bezier(0.2, 0.8, 0.2, 1);
}
.reveal {
  animation: reveal 0.2s ease-out;
}
@keyframes dialog-in {
  from {
    opacity: 0;
    transform: translateY(8px) scale(0.985);
  }
}
@keyframes reveal {
  from {
    opacity: 0;
    transform: translateY(-4px);
  }
}
@media (prefers-reduced-motion: reduce) {
  .dialog,
  .reveal {
    animation: none;
  }
}
/* The port boxes are four or five digits wide; the browser's spinner
   would cover half of that. */
.no-spin::-webkit-inner-spin-button,
.no-spin::-webkit-outer-spin-button {
  -webkit-appearance: none;
  margin: 0;
}
.no-spin {
  -moz-appearance: textfield;
  appearance: textfield;
}
</style>
