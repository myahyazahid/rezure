<script setup lang="ts">
import { computed } from 'vue'
import { useRoute } from 'vue-router'
import { stickerStyle, stickerUrl, useDecorationsStore } from '@/stores/decorations'

const store = useDecorationsStore()
const route = useRoute()

// A downloaded sticker whose image isn't there (yet, or any more) has nothing
// to draw; leaving it out beats a broken-image icon over the window.
const drawn = computed(() => store.stickers.filter((s) => stickerUrl(s.kind) !== ''))

// Hidden on the Decorations page itself: there the preview is the place to
// see them, and the real ones would sit on top of the editor.
const shown = computed(
  () => store.visible && drawn.value.length > 0 && route.path !== '/decorations',
)
</script>

<template>
  <!-- Above the page, below modals (z-50), and never in the way of a click. -->
  <div
    v-if="shown"
    aria-hidden="true"
    class="pointer-events-none fixed inset-0 z-40 overflow-hidden"
  >
    <img
      v-for="s in drawn"
      :key="s.id"
      :src="stickerUrl(s.kind)"
      alt=""
      draggable="false"
      class="sticker"
      :style="stickerStyle(s)"
    />
  </div>
</template>
