import { computed, ref } from 'vue'

/** What has to be typed before a destructive action unlocks. A fixed word
 *  rather than the thing's name: database and project names here are long
 *  and look alike (`fmt_new`, `fmt_staging_new`), and the point is a
 *  deliberate pause, not a test of spelling. */
export const CONFIRM_WORD = 'CONFIRM'

/**
 * The "type CONFIRM to delete" field shared by the Delete dialogs.
 *
 * `typed` is what the input binds to. Whatever is typed shows, and counts,
 * in uppercase, so caps lock or a lowercase "confirm" doesn't leave the
 * button locked with no hint why.
 */
export function useTypedConfirm() {
  const raw = ref('')
  const typed = computed({
    get: () => raw.value,
    set: (value: string) => {
      raw.value = value.toUpperCase()
    },
  })
  const confirmed = computed(() => raw.value.trim() === CONFIRM_WORD)
  return { typed, confirmed }
}
