<script setup lang="ts">
import { computed } from 'vue'
import { useProjectsStore } from '@/stores/projects'
import { useDevServersStore } from '@/stores/devServers'
import TechIcon from '@/components/common/TechIcon.vue'

const props = defineProps<{
  projectId: string
  domain: string
  path: string
  phpVersion: string | null
  nodeVersion: string | null
  /** Served through a dev server (`serve: 'proxy'`) rather than from files. */
  devServer?: boolean
}>()

const store = useProjectsStore()
const devServers = useDevServersStore()

/** A `proxy` project's dev server, as the Open button shows it. */
const devStatus = computed(() => (props.devServer ? devServers.statusOf(props.projectId) : null))

const openTitle = computed(() => {
  const url = `http://${props.domain}`
  switch (devStatus.value) {
    case null:
      return `Open ${url}`
    case 'running':
      return `Open ${url} — its dev server is running`
    case 'starting':
      return 'Starting its dev server — the browser opens once it answers'
    default:
      return `Open ${url} — starts its dev server first`
  }
})

const ICON_BUTTON_CLASS =
  'flex h-7 w-7 shrink-0 items-center justify-center rounded-full transition'

/** An icon button whose feature is switched on (a live share, a pinned
 *  version) stays tinted in the accent colour, so the state is visible without a click. */
const ON_CLASS =
  'bg-accent-500/12 text-accent-600 hover:bg-accent-500/20 dark:bg-accent-500/20 dark:text-accent-300 dark:hover:bg-accent-500/30'

// A fixed-size icon button in every state (idle / starting / active) rather
// than growing into a URL chip when active: this sits in a list row whose
// Actions column has a fixed width (see ProjectsView.vue), and a chip wide
// enough for a `trycloudflare.com` URL plus copy/stop buttons overflowed
// past it into the Domain/Stack columns. The URL itself lives in
// ProjectShareModal, opened from here instead.
const isActive = computed(() => Boolean(store.shareUrls[props.projectId]))
const isStarting = computed(() => store.sharingFor === props.projectId)

function onShareClick() {
  if (isStarting.value) return
  if (isActive.value) {
    store.openShareModal(props.projectId)
  } else {
    store.shareProject(props.projectId)
  }
}
</script>

<template>
  <!-- One toolbar, like a service row's: Open is the raised primary action and
       the rest stay bare until hovered, instead of seven bordered bubbles. -->
  <div class="glass-inset flex shrink-0 items-center gap-0.5 rounded-full p-1">
    <!-- Same width in every state, so the row's actions column never shifts:
         a dev server starting swaps the globe for a spinner, and its state
         is a dot on the button rather than another word. -->
    <button
      type="button"
      class="glass-accent relative flex h-7 shrink-0 items-center gap-1.5 rounded-full px-3 text-[13px] font-semibold transition disabled:cursor-wait"
      :title="openTitle"
      :disabled="devStatus === 'starting'"
      @click="store.openSite(props.projectId)"
    >
      <span
        v-if="devStatus === 'starting'"
        class="h-3.5 w-3.5 animate-spin rounded-full border-2 border-current border-t-transparent"
      />
      <svg
        v-else
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        class="h-3.5 w-3.5"
      >
        <circle cx="12" cy="12" r="9" />
        <path
          stroke-linecap="round"
          d="M3 12h18M12 3c2.5 2.7 2.5 15.3 0 18M12 3c-2.5 2.7-2.5 15.3 0 18"
        />
      </svg>
      Open
      <span
        v-if="devStatus && devStatus !== 'starting'"
        class="absolute top-0.5 right-0.5 h-2 w-2 rounded-full ring-2 ring-white/70 dark:ring-black/40"
        :class="devStatus === 'running' ? 'bg-emerald-500' : 'bg-neutral-400/80'"
        :aria-label="devStatus === 'running' ? 'dev server running' : 'dev server stopped'"
      ></span>
    </button>

    <!-- Neutral like the other icon buttons while idle, filled with the accent once a
         share is live — the same on/off language as the PHP/Node pin
         buttons, so an active tunnel is visible without opening the modal. -->
    <button
      type="button"
      :class="[
        ICON_BUTTON_CLASS,
        'disabled:cursor-wait disabled:opacity-70',
        isActive ? ON_CLASS : 'glass-ghost',
      ]"
      :disabled="isStarting"
      :title="
        isStarting
          ? 'Starting share…'
          : isActive
            ? 'This project is shared publicly — click for the link'
            : 'Share this project publicly (Cloudflare Quick Tunnel)'
      "
      @click="onShareClick"
    >
      <span
        v-if="isStarting"
        class="h-3.5 w-3.5 animate-spin rounded-full border-2 border-current border-t-transparent"
      />
      <svg
        v-else
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        class="h-3.5 w-3.5"
      >
        <path
          stroke-linecap="round"
          stroke-linejoin="round"
          d="M12 3v12M12 3l-4 4M12 3l4 4M5 15v3a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-3"
        />
      </svg>
    </button>

    <button
      type="button"
      :class="[ICON_BUTTON_CLASS, 'glass-ghost']"
      :title="`Open ${props.path} in Explorer`"
      @click="store.openFolder(props.projectId)"
    >
      <svg
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        class="h-3.5 w-3.5"
      >
        <path
          stroke-linecap="round"
          stroke-linejoin="round"
          d="M3 7a2 2 0 0 1 2-2h3.6l2 2.5H19a2 2 0 0 1 2 2V17a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z"
        />
      </svg>
    </button>

    <!-- Runs `php -m` on demand, never on render: one process per project
         per page visit would be the wrong trade for a question nobody asked
         yet. -->
    <button
      type="button"
      :class="[ICON_BUTTON_CLASS, 'glass-ghost']"
      title="Check this project's PHP extension requirements"
      @click="store.runDoctor(props.projectId)"
    >
      <svg
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        class="h-3.5 w-3.5"
      >
        <path
          stroke-linecap="round"
          stroke-linejoin="round"
          d="M6 3v6a4 4 0 0 0 8 0V3M10 17a4 4 0 0 0 8 0v-2"
        />
        <circle cx="18" cy="13" r="2" />
      </svg>
    </button>

    <!-- Pins this project to its own PHP version, distinct from the global
         Switch. The PHP logo says which runtime at a glance: grey while it
         follows the global version, in PHP's own colour on an accent fill
         once pinned, so a pinned project shows without opening the modal. -->
    <button
      type="button"
      :class="[ICON_BUTTON_CLASS, props.phpVersion ? ON_CLASS : 'glass-ghost']"
      :title="
        props.phpVersion
          ? `Pinned to PHP ${props.phpVersion} — click to change`
          : 'Pin this project to its own PHP version'
      "
      @click="store.openPhpVersionModal(props.projectId)"
    >
      <TechIcon id="php" :size="17" :mono="!props.phpVersion" />
    </button>

    <!-- Pins this project to its own Node.js version, same shape and
         behaviour as the PHP pin button above. It's what a terminal opened
         from this card resolves node/npm/npx as, and what the project's dev
         server runs on (see ProjectNodeVersionModal.vue). -->
    <button
      type="button"
      :class="[ICON_BUTTON_CLASS, props.nodeVersion ? ON_CLASS : 'glass-ghost']"
      :title="
        props.nodeVersion
          ? `Pinned to Node.js ${props.nodeVersion} — click to change`
          : 'Pin this project to its own Node.js version'
      "
      @click="store.openNodeVersionModal(props.projectId)"
    >
      <TechIcon id="node" :size="14" :mono="!props.nodeVersion" />
    </button>

    <button
      type="button"
      :class="[ICON_BUTTON_CLASS, 'glass-ghost']"
      title="Open a terminal in this folder"
      @click="store.openTerminal(props.projectId)"
    >
      <svg
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        class="h-3.5 w-3.5"
      >
        <rect x="3" y="4" width="18" height="16" rx="2" />
        <path stroke-linecap="round" stroke-linejoin="round" d="m7 9 3 3-3 3M13 15h4" />
      </svg>
    </button>
  </div>
</template>
