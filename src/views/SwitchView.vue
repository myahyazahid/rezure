<script setup lang="ts">
import { ref } from 'vue'
import { usePhpStore } from '@/stores/php'
import RuntimeSwitchRow from '@/components/services/RuntimeSwitchRow.vue'

const phpStore = usePhpStore()

// Placeholder installed-version lists for runtimes Rezure doesn't manage yet
// (only PHP is real, backed by `usePhpStore`) — mirrors the
// `ServiceLogPanel.vue` pattern of shipping the UI ahead of the backend.
const NODE_VERSIONS = ['22.6.0', '20.15.1', '18.20.4']
const nodeActive = ref(NODE_VERSIONS[0]!)

const MYSQL_VERSIONS = ['8.0.35', '8.0.34', '5.7.44']
const mysqlActive = ref(MYSQL_VERSIONS[0]!)

const COMPOSER_VERSIONS = ['2.7.7', '2.6.6']
const composerActive = ref(COMPOSER_VERSIONS[0]!)

const PYTHON_VERSIONS = ['3.12.4', '3.11.9', '3.10.14']
const pythonActive = ref(PYTHON_VERSIONS[0]!)
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
        class="flex shrink-0 items-center gap-2 rounded-full border border-neutral-200 bg-white/70 px-5 py-2.5 text-sm font-semibold text-neutral-700 transition hover:bg-white dark:border-neutral-700 dark:bg-neutral-900/60 dark:text-neutral-200 dark:hover:bg-neutral-800"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" class="h-4 w-4">
          <path stroke-linecap="round" d="M12 5v14M5 12h14" />
        </svg>
        Install version
      </button>
    </div>

    <div class="mt-5 flex flex-col gap-2.5">
      <RuntimeSwitchRow
        icon="P"
        name="PHP"
        :active-version="phpStore.active?.version ?? '—'"
        :installed-count="phpStore.versions.length"
        :versions="phpStore.versions.map((v) => v.version)"
        @select="phpStore.setActive"
      />
      <RuntimeSwitchRow
        icon="N"
        name="Node.js"
        :active-version="nodeActive"
        :installed-count="NODE_VERSIONS.length"
        :versions="NODE_VERSIONS"
        @select="(v) => (nodeActive = v)"
      />
      <RuntimeSwitchRow
        icon="M"
        name="MySQL"
        :active-version="mysqlActive"
        :installed-count="MYSQL_VERSIONS.length"
        :versions="MYSQL_VERSIONS"
        @select="(v) => (mysqlActive = v)"
      />
      <RuntimeSwitchRow
        icon="C"
        name="Composer"
        :active-version="composerActive"
        :installed-count="COMPOSER_VERSIONS.length"
        :versions="COMPOSER_VERSIONS"
        @select="(v) => (composerActive = v)"
      />
      <RuntimeSwitchRow
        icon="P"
        name="Python"
        :active-version="pythonActive"
        :installed-count="PYTHON_VERSIONS.length"
        :versions="PYTHON_VERSIONS"
        @select="(v) => (pythonActive = v)"
      />
    </div>

    <div
      class="mt-4 rounded-2xl border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-700 dark:border-red-500/30 dark:bg-red-500/10 dark:text-red-300"
    >
      Switching a runtime restarts the services that depend on it — PHP
      {{ phpStore.active?.version ?? '—' }} · Node.js {{ nodeActive }} · MySQL {{ mysqlActive }} ·
      Composer {{ composerActive }} · Python {{ pythonActive }}
    </div>
  </section>
</template>
