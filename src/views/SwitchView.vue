<script setup lang="ts">
import { computed, onMounted } from 'vue'
import { usePhpStore } from '@/stores/php'
import { useBinariesStore } from '@/stores/binaries'
import { useComposerStore } from '@/stores/composer'
import RuntimeSwitchRow, {
  type RuntimeVersionEntry,
} from '@/components/services/RuntimeSwitchRow.vue'

const phpStore = usePhpStore()
const binariesStore = useBinariesStore()
const composerStore = useComposerStore()

onMounted(() => {
  composerStore.fetchStatus()
})

const phpVersions = computed<RuntimeVersionEntry[]>(() =>
  phpStore.versions.map((v) => ({ id: v.id, version: v.version, installed: v.installed })),
)
const phpInstalledCount = computed(() => phpStore.versions.filter((v) => v.installed).length)

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
    <div>
      <h1 class="text-[28px] leading-tight font-bold tracking-tight">Switch</h1>
      <p class="mt-1 text-sm text-neutral-500">
        Pick the runtime version each new shell and vhost should use.
      </p>
    </div>

    <p v-if="phpStore.error" class="mt-3 text-sm text-red-600 dark:text-red-400">
      {{ phpStore.error }}
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
        @select="phpStore.setActive"
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

    <p class="mt-4 text-xs text-neutral-400">
      Node.js and Python aren't available yet — Rezure doesn't bundle a portable runtime for either,
      so there's nothing installable to switch between.
    </p>
  </section>
</template>
