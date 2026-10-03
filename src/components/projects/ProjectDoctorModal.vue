<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useProjectsStore } from '@/stores/projects'
import { usePhpStore } from '@/stores/php'
import { useServicesStore } from '@/stores/services'
import { useBinariesStore } from '@/stores/binaries'
import LicenseConsentModal from '@/components/common/LicenseConsentModal.vue'
import { useLicensedInstall } from '@/composables/useLicensedInstall'

const store = useProjectsStore()
const phpStore = usePhpStore()
const servicesStore = useServicesStore()
const binariesStore = useBinariesStore()

const project = computed(() => store.projects.find((p) => p.id === store.doctorFor) ?? null)
const result = computed(() => store.diagnosis)
const loading = computed(() => store.doctorFor !== null && !result.value && !store.doctorError)

/** The one number that decides what this modal is saying. */
const missingCount = computed(() => result.value?.missing.length ?? 0)

/** Names installed during this visit — what the "restart PHP" note is for. */
const justInstalled = ref<string[]>([])

const tls = computed(() => phpStore.tls)

/** A bundle is the fix whenever one is missing, and whenever a certificate
 *  failed to verify — a stale bundle can do that too. Offline alone isn't a
 *  reason: it says nothing about certificates. */
const tlsNeedsBundle = computed(
  () =>
    !!tls.value &&
    tls.value.outcome !== 'verified' &&
    (tls.value.outcome === 'untrusted' || !tls.value.bundleInstalled),
)

/** Set once the bundle is fetched from here: `created` decides whether the
 *  running PHP needs a restart to see it. */
const bundleFix = ref<{ created: boolean } | null>(null)

// The HTTPS half is about the PHP, not the project, and waits on the
// network — so it runs beside the extension check rather than inside it.
watch(
  () => store.doctorFor,
  (id) => {
    bundleFix.value = null
    if (id) phpStore.checkTls()
  },
  { immediate: true },
)

async function fixBundle() {
  const created = await phpStore.updateCaBundle()
  if (created === null) return
  bundleFix.value = { created }
  // Re-ask rather than assume: the probe is what actually knows.
  await phpStore.checkTls()
}

// The catalog is per PHP branch, so it can only be asked for once the check
// has said which PHP it was talking about.
watch(
  () => result.value?.phpVersion,
  (phpVersion) => {
    justInstalled.value = []
    if (phpVersion) phpStore.fetchExtensions(phpVersion)
  },
  { immediate: true },
)

/** The catalog entry for a missing extension, when Rezure can install it. */
function installable(name: string) {
  return phpStore.extensions.find((e) => e.id === name && e.available && !e.installed) ?? null
}

const mail = computed(() => result.value?.mail ?? null)

/** The `.env` already points where Mailpit listens — only whether it's
 *  running is left. */
const mailConfigured = computed(() => !!mail.value?.hostReachable && !!mail.value?.portMatches)

const mailBusy = ref(false)
const mailError = ref<string | null>(null)

/** Installs Mailpit if it isn't yet, starts it, then re-checks so the
 *  section reads from what's actually running. */
async function startMailpit() {
  mailBusy.value = true
  mailError.value = null
  try {
    if (!mail.value?.mailpitInstalled) await binariesStore.install('mailpit')
    await servicesStore.start('mailpit')
    if (store.doctorFor) await store.runDoctor(store.doctorFor)
  } catch (e) {
    mailError.value = typeof e === 'string' ? e : 'Mailpit could not be started.'
  } finally {
    mailBusy.value = false
  }
}

async function openInbox() {
  mailError.value = null
  try {
    await servicesStore.openUi('mailpit')
  } catch (e) {
    mailError.value = typeof e === 'string' ? e : 'Mailpit could not be opened.'
  }
}

const sql = computed(() => result.value?.sqlServer ?? null)

/** Everything a request needs to reach SQL Server is in place. The
 *  certificate note isn't part of this: whether it applies depends on the
 *  server, which only a real connect can tell. */
const sqlReady = computed(
  () =>
    !!sql.value &&
    sql.value.driverLoaded &&
    sql.value.odbcDriver !== null &&
    (!sql.value.usesLocaldb || (sql.value.localdbInstalled && !sql.value.portConflicts)),
)

/** ODBC Driver 18 refuses a certificate it can't verify, and the project
 *  hasn't switched trust on. LocalDB is fine either way — checked by
 *  connecting to it with and without. */
const sqlTrustNote = computed(
  () =>
    !!sql.value &&
    !sql.value.usesLocaldb &&
    sql.value.encryptsByDefault &&
    !sql.value.trustConfigured,
)

/** The ODBC Driver and LocalDB are Microsoft installers — license first. */
const licensed = useLicensedInstall()

async function installSqlPart(id: 'msodbcsql' | 'sqllocaldb') {
  if (await licensed.request(id)) await recheck()
}

async function confirmLicensed() {
  if (await licensed.confirm()) await recheck()
}

async function recheck() {
  if (store.doctorFor) await store.runDoctor(store.doctorFor)
}

const pg = computed(() => result.value?.postgres ?? null)

/** Rezure's PostgreSQL listens on 5432 and has the roles `postgres` and
 *  `root`; a `.env` pointed at this machine has to agree with both. */
const pgPortMismatch = computed(() => !!pg.value && pg.value.usesLocal && pg.value.port !== 5432)

/** Everything a request needs to reach PostgreSQL is in place. A server on
 *  another machine is the project's business — only the driver is checked. */
const pgReady = computed(
  () =>
    !!pg.value &&
    pg.value.driverLoaded &&
    (!pg.value.usesLocal ||
      (pg.value.postgresRunning && !pgPortMismatch.value && pg.value.knownRole)),
)

const pgBusy = ref(false)
const pgError = ref<string | null>(null)

/** Turns `pdo_pgsql` on, installs PostgreSQL, or starts it — whichever the
 *  row asked for — then re-checks against what's actually there. */
async function fixPostgres(step: 'driver' | 'install' | 'start') {
  pgBusy.value = true
  pgError.value = null
  try {
    if (step === 'driver') {
      const phpVersion = result.value?.phpVersion
      if (!phpVersion) return
      const ok = await phpStore.setBundledExtension(phpVersion, 'pdo_pgsql', true)
      if (!ok) return
      justInstalled.value = [...justInstalled.value, 'pdo_pgsql']
    } else if (step === 'install') {
      await binariesStore.install('postgres')
    } else {
      await servicesStore.start('postgres')
    }
    await recheck()
  } catch (e) {
    pgError.value = typeof e === 'string' ? e : 'PostgreSQL could not be set up.'
  } finally {
    pgBusy.value = false
  }
}

async function install(name: string) {
  const phpVersion = result.value?.phpVersion
  if (!phpVersion) return
  const ok = await phpStore.installExtension(name, phpVersion)
  if (!ok) return
  justInstalled.value = [...justInstalled.value, name]
  // Re-check rather than assume: `php -m` is the only thing that actually
  // knows whether the DLL loaded.
  if (store.doctorFor) await store.runDoctor(store.doctorFor)
}
</script>

<template>
  <div
    v-if="store.doctorFor"
    class="glass-scrim fixed inset-0 z-50 flex items-center justify-center p-4"
    @click.self="store.closeDoctor()"
  >
    <div class="glass-strong w-full max-w-lg rounded-2xl p-6">
      <h2 class="text-lg font-bold text-neutral-900 dark:text-neutral-100">
        Requirements check
        <span v-if="project" class="font-normal text-neutral-400">· {{ project.name }}</span>
      </h2>
      <p class="mt-1 text-sm text-neutral-500">
        Every <code class="font-mono">ext-*</code> in this project's
        <code class="font-mono">composer.json</code>, checked against the PHP that serves it —
        whether that PHP can make HTTPS calls, where the project's mail goes, and, for a project on
        SQL Server or PostgreSQL, whether it can connect.
      </p>

      <p v-if="loading" class="mt-5 text-sm text-neutral-500">Asking PHP…</p>

      <p
        v-else-if="store.doctorError"
        class="mt-5 rounded-xl bg-amber-100/50 px-3 py-2 text-sm text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
      >
        {{ store.doctorError }}
      </p>

      <template v-else-if="result">
        <!-- No composer.json and no ext-* are different findings, and both
             are results rather than failures: most of www is WordPress and
             static folders. -->
        <p v-if="!result.hasComposerJson" class="mt-5 text-sm text-neutral-500">
          No <code class="font-mono">composer.json</code> here, so there's nothing to check.
        </p>
        <p v-else-if="result.extensions.length === 0" class="mt-5 text-sm text-neutral-500">
          This project doesn't require any PHP extension explicitly.
        </p>

        <template v-else>
          <p
            class="mt-5 rounded-xl px-3 py-2 text-sm"
            :class="
              missingCount > 0
                ? 'bg-amber-100/50 text-amber-900 dark:bg-amber-500/10 dark:text-amber-200'
                : 'bg-emerald-100/50 text-emerald-800 dark:bg-emerald-500/10 dark:text-emerald-300'
            "
          >
            <template v-if="missingCount > 0">
              PHP {{ result.phpVersion }} is missing <strong>{{ result.missing.join(', ') }}</strong
              >. That's the kind of gap that shows up as a blank 500 with the reason only in
              <code class="font-mono">laravel.log</code>.
            </template>
            <template v-else>
              PHP {{ result.phpVersion }} has everything this project asks for.
            </template>
          </p>

          <ul class="mt-4 flex flex-col gap-1.5">
            <li
              v-for="check in result.extensions"
              :key="check.name"
              class="flex items-center gap-2 text-sm"
            >
              <span
                class="flex h-5 w-5 shrink-0 items-center justify-center rounded-full text-[11px] font-bold"
                :class="
                  check.loaded
                    ? 'bg-emerald-500/15 text-emerald-700 dark:bg-emerald-500/15 dark:text-emerald-400'
                    : check.devOnly
                      ? 'glass-inset text-neutral-500 dark:text-neutral-400'
                      : 'bg-amber-500/15 text-amber-700 dark:bg-amber-500/15 dark:text-amber-300'
                "
              >
                {{ check.loaded ? '✓' : '!' }}
              </span>
              <code class="font-mono text-neutral-800 dark:text-neutral-200">{{ check.name }}</code>
              <span v-if="check.devOnly" class="text-xs text-neutral-400">dev only</span>

              <!-- Only for the ones Rezure has a checksum-verified build of;
                   everything else stays a diagnosis, not a dead button. -->
              <button
                v-if="!check.loaded && installable(check.name)"
                type="button"
                class="glass-btn ml-auto shrink-0 rounded-full px-3 py-1 text-xs font-semibold text-neutral-700 transition disabled:opacity-50 dark:text-neutral-200"
                :disabled="phpStore.installingExtension !== null"
                @click="install(check.name)"
              >
                <template v-if="phpStore.installingExtension === check.name">Installing…</template>
                <template v-else>Install {{ installable(check.name)?.version }}</template>
              </button>
            </li>
          </ul>

          <!-- The next step, not just the diagnosis: enabling an extension
               is a line in the settings folder, and that folder is the one
               place an edit survives a restart and a version switch. -->
          <p v-if="missingCount > 0" class="mt-4 text-xs text-neutral-500">
            Add <code class="font-mono">extension={{ result.missing[0] }}</code> to an
            <code class="font-mono">.ini</code> file in your settings folder, then restart PHP. If
            the DLL isn't in the build, it has to be installed first.
          </p>
        </template>
      </template>

      <!-- Mail: only when the .env sends to an SMTP server meant to be on
           this machine. `log` or a real provider is a choice, not a gap. -->
      <div v-if="mail && !store.doctorError" class="glass-divider mt-5 border-t pt-4">
        <template v-if="mailConfigured && mail.mailpitRunning">
          <p class="flex items-center gap-2 text-sm text-neutral-700 dark:text-neutral-300">
            <span
              class="flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-emerald-500/15 text-[11px] font-bold text-emerald-700 dark:bg-emerald-500/15 dark:text-emerald-400"
              >✓</span
            >
            Mail this project sends is caught by Mailpit.
            <button
              type="button"
              class="glass-btn ml-auto shrink-0 rounded-full px-3 py-1 text-xs font-semibold text-neutral-700 transition dark:text-neutral-200"
              @click="openInbox"
            >
              Open inbox
            </button>
          </p>
        </template>

        <div
          v-else
          class="rounded-xl bg-amber-100/50 px-3 py-2 text-sm text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
        >
          <p v-if="!mail.hostReachable">
            <code class="font-mono">MAIL_HOST={{ mail.host }}</code> is Laravel Sail's Docker
            hostname and doesn't resolve outside Docker. Set
            <code class="font-mono">MAIL_HOST=127.0.0.1</code> in
            <code class="font-mono">.env</code> to send to Mailpit here.
          </p>
          <p v-else-if="!mail.portMatches">
            This project sends mail to
            <code class="font-mono">{{ mail.host }}:{{ mail.port ?? 2525 }}</code
            >, but Mailpit listens on 1025. Set <code class="font-mono">MAIL_PORT=1025</code> in
            <code class="font-mono">.env</code> to catch it.
          </p>
          <p v-else>
            This project sends mail to
            <code class="font-mono">{{ mail.host }}:{{ mail.port }}</code
            >, but Mailpit isn't running, so sending fails.
          </p>
          <button
            v-if="!mail.mailpitRunning"
            type="button"
            class="glass-btn mt-2 rounded-full px-3 py-1 text-xs font-semibold text-amber-900 transition disabled:opacity-50 dark:text-amber-200"
            :disabled="mailBusy"
            @click="startMailpit"
          >
            <template v-if="mailBusy">Starting…</template>
            <template v-else-if="mail.mailpitInstalled">Start Mailpit</template>
            <template v-else>Install and start Mailpit</template>
          </button>
        </div>

        <p v-if="mailError" class="mt-2 text-xs text-red-600 dark:text-red-400">{{ mailError }}</p>
      </div>

      <!-- SQL Server: only when the .env says DB_CONNECTION=sqlsrv. A Laravel
           app on SQL Server declares none of this in composer.json, so the
           extension list above can't catch any of it. -->
      <div v-if="sql && !store.doctorError" class="glass-divider mt-5 border-t pt-4">
        <p
          v-if="sqlReady"
          class="flex items-center gap-2 text-sm text-neutral-700 dark:text-neutral-300"
        >
          <span
            class="flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-emerald-500/15 text-[11px] font-bold text-emerald-700 dark:bg-emerald-500/15 dark:text-emerald-400"
            >✓</span
          >
          <template v-if="sql.usesLocaldb">
            SQL Server is ready — this project uses Rezure's LocalDB.
          </template>
          <template v-else>
            PHP can reach SQL Server at <code class="font-mono">{{ sql.host }}</code
            >.
          </template>
        </p>

        <div v-else class="flex flex-col gap-2">
          <div
            v-if="!sql.driverLoaded"
            class="rounded-xl bg-amber-100/50 px-3 py-2 text-sm text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
          >
            PHP {{ result?.phpVersion }} doesn't load <code class="font-mono">pdo_sqlsrv</code>,
            which Laravel's <code class="font-mono">sqlsrv</code> connection needs.
            <button
              v-if="installable('pdo_sqlsrv')"
              type="button"
              class="glass-btn mt-2 block rounded-full px-3 py-1 text-xs font-semibold text-amber-900 transition disabled:opacity-50 dark:text-amber-200"
              :disabled="phpStore.installingExtension !== null"
              @click="install('pdo_sqlsrv')"
            >
              <template v-if="phpStore.installingExtension === 'pdo_sqlsrv'">Installing…</template>
              <template v-else
                >Install pdo_sqlsrv {{ installable('pdo_sqlsrv')?.version }}</template
              >
            </button>
          </div>

          <div
            v-if="!sql.odbcDriver"
            class="rounded-xl bg-amber-100/50 px-3 py-2 text-sm text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
          >
            The Microsoft ODBC Driver for SQL Server isn't installed. PHP loads
            <code class="font-mono">pdo_sqlsrv</code> without it, but every connection fails with
            "This extension requires the Microsoft ODBC Driver".
            <button
              type="button"
              class="glass-btn mt-2 block rounded-full px-3 py-1 text-xs font-semibold text-amber-900 transition disabled:opacity-50 dark:text-amber-200"
              :disabled="binariesStore.isInstalling('msodbcsql')"
              @click="installSqlPart('msodbcsql')"
            >
              {{ binariesStore.isInstalling('msodbcsql') ? 'Installing…' : 'Install ODBC Driver' }}
            </button>
          </div>

          <div
            v-if="sql.usesLocaldb && !sql.localdbInstalled"
            class="rounded-xl bg-amber-100/50 px-3 py-2 text-sm text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
          >
            <code class="font-mono">DB_HOST</code> points at Rezure's LocalDB, but LocalDB isn't
            installed.
            <button
              type="button"
              class="glass-btn mt-2 block rounded-full px-3 py-1 text-xs font-semibold text-amber-900 transition disabled:opacity-50 dark:text-amber-200"
              :disabled="binariesStore.isInstalling('sqllocaldb')"
              @click="installSqlPart('sqllocaldb')"
            >
              {{ binariesStore.isInstalling('sqllocaldb') ? 'Installing…' : 'Install LocalDB' }}
            </button>
          </div>
          <div
            v-else-if="sql.usesLocaldb && sql.portConflicts"
            class="rounded-xl bg-amber-100/50 px-3 py-2 text-sm text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
          >
            LocalDB answers on a named pipe, not a port, but Laravel adds
            <code class="font-mono">DB_PORT</code> — or its default, 1433 — to the host unless it's
            empty. Set <code class="font-mono">DB_PORT=</code> (nothing after the
            <code class="font-mono">=</code>) in <code class="font-mono">.env</code>.
          </div>
        </div>

        <!-- Not a failure Rezure can see from here — only a real connect
             shows whether the server's certificate is one the driver trusts. -->
        <p v-if="sqlTrustNote" class="mt-2 text-xs text-neutral-500">
          ODBC Driver 18 encrypts by default and refuses a self-signed certificate. If connecting
          fails with a certificate error, uncomment
          <code class="font-mono">'trust_server_certificate'</code> in
          <code class="font-mono">config/database.php</code> and set
          <code class="font-mono">DB_TRUST_SERVER_CERTIFICATE=true</code> — the env var alone does
          nothing, because Laravel ships that line commented out.
        </p>
        <p v-if="licensed.error.value" class="mt-2 text-xs text-red-600 dark:text-red-400">
          {{ licensed.error.value }}
        </p>
      </div>

      <!-- PostgreSQL: only when the .env says DB_CONNECTION=pgsql. pdo_pgsql
           ships with PHP but is off by default — the usual "could not find
           driver" — and nothing in composer.json says a project needs it. -->
      <div v-if="pg && !store.doctorError" class="glass-divider mt-5 border-t pt-4">
        <p
          v-if="pgReady"
          class="flex items-center gap-2 text-sm text-neutral-700 dark:text-neutral-300"
        >
          <span
            class="flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-emerald-500/15 text-[11px] font-bold text-emerald-700 dark:bg-emerald-500/15 dark:text-emerald-400"
            >✓</span
          >
          <template v-if="pg.usesLocal">
            PostgreSQL is ready — this project uses Rezure's server as
            <code class="font-mono">{{ pg.username }}</code
            >.
          </template>
          <template v-else>
            PHP has the PostgreSQL driver for <code class="font-mono">{{ pg.host }}</code
            >.
          </template>
        </p>

        <div v-else class="flex flex-col gap-2">
          <div
            v-if="!pg.driverLoaded"
            class="rounded-xl bg-amber-100/50 px-3 py-2 text-sm text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
          >
            PHP {{ result?.phpVersion }} doesn't load <code class="font-mono">pdo_pgsql</code>, so
            Laravel fails with "could not find driver". It ships with PHP, just switched off.
            <button
              type="button"
              class="glass-btn mt-2 block rounded-full px-3 py-1 text-xs font-semibold text-amber-900 transition disabled:opacity-50 dark:text-amber-200"
              :disabled="pgBusy"
              @click="fixPostgres('driver')"
            >
              Turn on pdo_pgsql
            </button>
          </div>

          <template v-if="pg.usesLocal">
            <div
              v-if="!pg.postgresInstalled"
              class="rounded-xl bg-amber-100/50 px-3 py-2 text-sm text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
            >
              This project connects to PostgreSQL on this machine, but it isn't installed.
              <button
                type="button"
                class="glass-btn mt-2 block rounded-full px-3 py-1 text-xs font-semibold text-amber-900 transition disabled:opacity-50 dark:text-amber-200"
                :disabled="pgBusy || binariesStore.isInstalling('postgres')"
                @click="fixPostgres('install')"
              >
                {{ binariesStore.isInstalling('postgres') ? 'Installing…' : 'Install PostgreSQL' }}
              </button>
            </div>
            <div
              v-else-if="!pg.postgresRunning"
              class="rounded-xl bg-amber-100/50 px-3 py-2 text-sm text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
            >
              PostgreSQL isn't running, so connecting fails.
              <button
                type="button"
                class="glass-btn mt-2 block rounded-full px-3 py-1 text-xs font-semibold text-amber-900 transition disabled:opacity-50 dark:text-amber-200"
                :disabled="pgBusy"
                @click="fixPostgres('start')"
              >
                {{ pgBusy ? 'Starting…' : 'Start PostgreSQL' }}
              </button>
            </div>
            <div
              v-if="pgPortMismatch"
              class="rounded-xl bg-amber-100/50 px-3 py-2 text-sm text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
            >
              <code class="font-mono">DB_PORT={{ pg.port }}</code
              >, but Rezure's PostgreSQL listens on 5432. Set
              <code class="font-mono">DB_PORT=5432</code> in <code class="font-mono">.env</code>.
            </div>
            <div
              v-if="!pg.knownRole"
              class="rounded-xl bg-amber-100/50 px-3 py-2 text-sm text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
            >
              Rezure's PostgreSQL has no role
              <code class="font-mono">{{ pg.username }}</code
              >. Set <code class="font-mono">DB_USERNAME=root</code> (or
              <code class="font-mono">postgres</code>) — any password works, the local server trusts
              this machine.
            </div>
          </template>
        </div>
        <p v-if="pgError" class="mt-2 text-xs text-red-600 dark:text-red-400">{{ pgError }}</p>
      </div>

      <!-- HTTPS: independent of composer.json, so shown for every project
           once the PHP question itself could be asked. -->
      <div v-if="!store.doctorError" class="glass-divider mt-5 border-t pt-4">
        <p v-if="phpStore.checkingTls" class="text-sm text-neutral-500">Testing HTTPS…</p>

        <template v-else-if="tls">
          <p
            v-if="tls.outcome === 'verified'"
            class="flex items-center gap-2 text-sm text-neutral-700 dark:text-neutral-300"
          >
            <span
              class="flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-emerald-500/15 text-[11px] font-bold text-emerald-700 dark:bg-emerald-500/15 dark:text-emerald-400"
              >✓</span
            >
            HTTPS calls from PHP verify certificates.
          </p>

          <div
            v-else-if="tlsNeedsBundle"
            class="rounded-xl bg-amber-100/50 px-3 py-2 text-sm text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
          >
            <p>
              <template v-if="tls.bundleInstalled">
                PHP couldn't verify an HTTPS certificate. The CA bundle may be out of date.
              </template>
              <template v-else>
                No CA bundle is installed, so PHP can't verify any HTTPS certificate.
              </template>
              Outbound calls (Laravel's Http client, Guzzle, any API) fail with
              <code class="font-mono">cURL error 60</code>.
            </p>
            <p v-if="tls.detail" class="mt-1 font-mono text-xs opacity-80">{{ tls.detail }}</p>
            <button
              type="button"
              class="glass-btn mt-2 rounded-full px-3 py-1 text-xs font-semibold text-amber-900 transition disabled:opacity-50 dark:text-amber-200"
              :disabled="phpStore.updatingCaBundle"
              @click="fixBundle"
            >
              <template v-if="phpStore.updatingCaBundle">Downloading…</template>
              <template v-else-if="tls.bundleInstalled">Update CA bundle</template>
              <template v-else>Download CA bundle</template>
            </button>
          </div>

          <p v-else-if="tls.outcome === 'unreachable'" class="text-sm text-neutral-500">
            Couldn't reach the internet to test HTTPS<template v-if="tls.detail">
              ({{ tls.detail }})</template
            >. The CA bundle is installed.
          </p>

          <p v-else class="text-sm text-neutral-500">
            HTTPS check skipped<template v-if="tls.detail">: {{ tls.detail }}</template
            >.
          </p>
        </template>

        <p
          v-if="bundleFix"
          class="mt-3 rounded-xl bg-emerald-100/50 px-3 py-2 text-xs text-emerald-800 dark:bg-emerald-500/10 dark:text-emerald-300"
        >
          <template v-if="bundleFix.created">
            CA bundle installed. Restart the PHP service for running sites to pick it up — the copy
            already serving requests read its configuration before the bundle existed.
          </template>
          <template v-else>CA bundle updated. Running sites use it from the next request.</template>
        </p>
      </div>

      <p
        v-if="justInstalled.length"
        class="mt-4 rounded-xl bg-emerald-100/50 px-3 py-2 text-xs text-emerald-800 dark:bg-emerald-500/10 dark:text-emerald-300"
      >
        Installed <strong>{{ justInstalled.join(', ') }}</strong
        >. Restart the PHP service for running sites to pick it up — the copy already serving
        requests read its configuration when it started.
      </p>

      <p
        v-if="phpStore.error"
        class="mt-4 rounded-xl bg-amber-100/50 px-3 py-2 text-xs text-amber-900 dark:bg-amber-500/10 dark:text-amber-200"
      >
        {{ phpStore.error }}
      </p>

      <div class="mt-6 flex items-center justify-end gap-2">
        <button
          v-if="missingCount > 0"
          type="button"
          class="glass-btn rounded-full px-4 py-2 text-sm font-semibold text-neutral-700 transition dark:text-neutral-200"
          @click="phpStore.openConfigDir()"
        >
          Open settings folder
        </button>
        <button
          type="button"
          class="glass-accent rounded-full px-4 py-2 text-sm font-semibold transition"
          @click="store.closeDoctor()"
        >
          Close
        </button>
      </div>
    </div>

    <LicenseConsentModal
      v-if="licensed.pending.value"
      :pkg="licensed.pending.value"
      @confirm="confirmLicensed"
      @close="licensed.cancel"
    />
  </div>
</template>
