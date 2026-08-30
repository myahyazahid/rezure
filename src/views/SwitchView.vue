<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { usePhpStore } from '@/stores/php'
import { useBinariesStore } from '@/stores/binaries'
import { useServicesStore } from '@/stores/services'
import { useComposerStore } from '@/stores/composer'
import RuntimeSwitchRow, {
  type RuntimeVersionEntry,
} from '@/components/services/RuntimeSwitchRow.vue'
import InstallPhpVersionModal from '@/components/services/InstallPhpVersionModal.vue'
import PhpPathLinkCard from '@/components/services/PhpPathLinkCard.vue'

const phpStore = usePhpStore()
const binariesStore = useBinariesStore()
const servicesStore = useServicesStore()
const composerStore = useComposerStore()

const showInstallModal = ref(false)

/**
 * Switching also reloads the running PHP service on the backend, so the
 * service list is stale afterwards — its version badge and uptime both
 * changed. Nothing polls it, so it's refreshed here rather than leaving
 * the Services page showing the version that was running a moment ago.
 */
async function switchPhp(id: string) {
  const result = await phpStore.setActive(id)
  if (result?.restarted) await servicesStore.fetchAll()
}

onMounted(() => {
  composerStore.fetchStatus()
  phpStore.fetchDropInDir()
  phpStore.fetchPathStatus()
})

// PHP versions are discovered on disk, so everything listed is installed by
// definition — installing a new one is the modal's job, not the dropdown's.
const phpVersions = computed<RuntimeVersionEntry[]>(() =>
  phpStore.versions.map((v) => ({ id: v.id, version: v.version, installed: v.installed })),
)
const phpInstalledCount = computed(() => phpStore.versions.length)
const customPhpCount = computed(() => phpStore.versions.filter((v) => !v.managed).length)

const mariadb = computed(() => binariesStore.binaries.find((b) => b.id === 'mariadb') ?? null)
const mariadbVersions = computed<RuntimeVersionEntry[]>(() =>
  mariadb.value
    ? [{ id: 'mariadb', version: mariadb.value.version, installed: mariadb.value.installed }]
    : [],
)

const composerVersions = computed<RuntimeVersionEntry[]>(() => [
  { id: 'composer', version: 'latest', installed: composerStore.installed },
])
</script>

<template>
  <section>
    <div class="flex items-start justify-between gap-4">
      <div>
        <h1 class="text-[28px] leading-tight font-bold tracking-tight">Switch</h1>
        <p class="mt-1 text-sm text-neutral-500">
          Pick the runtime version each new shell and vhost should use.
        </p>
      </div>

      <button
        type="button"
        class="flex shrink-0 items-center gap-2 rounded-full bg-red-600 px-5 py-2.5 text-sm font-semibold text-white shadow-lg shadow-red-500/40 transition hover:bg-red-500"
        @click="showInstallModal = true"
      >
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2.5"
          class="h-4 w-4"
        >
          <path stroke-linecap="round" d="M12 5v14M5 12h14" />
        </svg>
        Install version
      </button>
    </div>

    <InstallPhpVersionModal v-if="showInstallModal" @close="showInstallModal = false" />

    <p v-if="phpStore.error" class="mt-3 text-sm text-red-600 dark:text-red-400">
      {{ phpStore.error }}
    </p>
    <p v-else-if="phpStore.notice" class="mt-3 text-sm text-emerald-600 dark:text-emerald-400">
      {{ phpStore.notice }}
    </p>
    <p v-if="composerStore.error" class="mt-3 text-sm text-red-600 dark:text-red-400">
      {{ composerStore.error }}
    </p>

    <div class="mt-5 flex flex-col gap-2.5">
      <RuntimeSwitchRow
        icon="P"
        name="PHP"
        :active-version="phpStore.active?.version ?? null"
        :installed-count="phpInstalledCount"
        :versions="phpVersions"
        :installing-id="phpStore.installingId"
        @select="switchPhp"
        @install="phpStore.install"
      />
      <RuntimeSwitchRow
        icon="M"
        name="MariaDB"
        :active-version="mariadb?.installed ? mariadb.version : null"
        :installed-count="mariadb?.installed ? 1 : 0"
        :versions="mariadbVersions"
        :installing-id="binariesStore.isInstalling('mariadb') ? 'mariadb' : null"
        @install="binariesStore.install('mariadb')"
      />
      <RuntimeSwitchRow
        icon="C"
        name="Composer"
        :active-version="composerStore.installed ? 'latest' : null"
        :installed-count="composerStore.installed ? 1 : 0"
        :versions="composerVersions"
        :installing-id="composerStore.installing ? 'composer' : null"
        @install="composerStore.install"
      />
      <RuntimeSwitchRow
        icon="N"
        name="Node.js"
        active-version=""
        :installed-count="0"
        :versions="[]"
        disabled
      />
      <RuntimeSwitchRow
        icon="P"
        name="Python"
        active-version=""
        :installed-count="0"
        :versions="[]"
        disabled
      />
    </div>

    <div class="mt-3">
      <PhpPathLinkCard />
    </div>

    <div class="mt-4 flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-neutral-400">
      <span v-if="customPhpCount > 0">
        {{ customPhpCount }} PHP {{ customPhpCount === 1 ? 'version was' : 'versions were' }} added
        by hand — Rezure didn't checksum those.
      </span>
      <span v-if="phpStore.dropInDir" class="flex items-center gap-1">
        Drop-in folder:
        <button
          type="button"
          class="truncate font-mono underline"
          :title="phpStore.dropInDir"
          @click="phpStore.openDropInDir"
        >
          {{ phpStore.dropInDir }}
        </button>
      </span>
    </div>

    <p class="mt-2 text-xs text-neutral-400">
      Node.js and Python aren't available yet — Rezure doesn't bundle a portable runtime for either,
      so there's nothing installable to switch between.
    </p>
  </section>
</template>
