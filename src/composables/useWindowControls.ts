import { getCurrentWindow } from '@tauri-apps/api/window'

/**
 * Window buttons for the custom title bar — the OS chrome is disabled
 * (`decorations: false`), so minimize/maximize/close are driven from here.
 */
export function useWindowControls() {
  const appWindow = getCurrentWindow()

  return {
    minimize: () => appWindow.minimize(),
    toggleMaximize: () => appWindow.toggleMaximize(),
    close: () => appWindow.close(),
  }
}
