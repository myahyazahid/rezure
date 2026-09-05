import { ref, watchEffect } from 'vue'

const THEME_KEY = 'rezure-theme'
type Theme = 'light' | 'dark'

/** Light unless the user has picked otherwise — deliberately not following
 *  the OS, so a first launch looks the same for everyone. A choice made from
 *  the title bar is remembered from then on. */
function getPreferredTheme(): Theme {
  try {
    const stored = localStorage.getItem(THEME_KEY)
    if (stored === 'light' || stored === 'dark') return stored
  } catch {
    // localStorage unavailable — fall through to the default
  }
  return 'light'
}

const theme = ref<Theme>(getPreferredTheme())

watchEffect(() => {
  document.documentElement.classList.toggle('dark', theme.value === 'dark')
  try {
    localStorage.setItem(THEME_KEY, theme.value)
  } catch {
    // ignore persistence failures (private browsing, etc.)
  }
})

export function useTheme() {
  function toggle() {
    theme.value = theme.value === 'dark' ? 'light' : 'dark'
  }

  return { theme, toggle }
}
