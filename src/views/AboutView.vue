<script setup lang="ts">
import { useDonateStore } from '@/stores/donate'

const donateStore = useDonateStore()
</script>

<template>
  <section>
    <h1 class="text-2xl font-semibold text-neutral-900 dark:text-neutral-100">About Rezure</h1>
    <p class="mt-1 text-sm text-neutral-500">
      A modern, lightweight local development environment manager for Windows — inspired by
      Laragon, built with Tauri + Rust + Vue 3.
    </p>

    <div
      class="mt-6 rounded-2xl border border-neutral-200 bg-white p-5 text-sm text-neutral-700 dark:border-neutral-800 dark:bg-neutral-900/60 dark:text-neutral-300"
    >
      <!-- TODO: replace with the project's real story — this is a placeholder
           paragraph, not maintainer-authored content. -->
      <p>
        Rezure is free and open-source, built to make local PHP development on Windows painless —
        start Apache/Nginx, PHP, and MySQL with one click, manage local projects, and switch
        runtime versions without hunting through config files by hand.
      </p>
    </div>

    <!-- TODO: repository field in src-tauri/Cargo.toml is still empty — wire
         this button to the real repo URL once one exists. -->
    <button
      type="button"
      class="mt-4 rounded-full bg-red-600 px-4 py-2 text-sm font-semibold text-white transition hover:bg-red-500"
      @click="donateStore.openLink('REPLACE_WITH_REPO_URL')"
    >
      View on GitHub
    </button>
  </section>
</template>
