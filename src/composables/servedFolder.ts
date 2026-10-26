/**
 * `docroot` relative to `projectPath` (`public`, `src\web`), or `''` when
 * nginx serves the project folder itself. Both come from Rust already
 * resolved — this only shortens them for display.
 */
export function servedFolder(projectPath: string, docroot: string): string {
  const trim = (path: string) => path.replace(/[\\/]+$/, '')
  const base = trim(projectPath)
  const served = trim(docroot)
  if (served.toLowerCase() === base.toLowerCase()) return ''
  if (
    served.toLowerCase().startsWith(base.toLowerCase()) &&
    /[\\/]/.test(served[base.length] ?? '')
  ) {
    return served.slice(base.length + 1)
  }
  return served.split(/[\\/]/).pop() ?? ''
}

/** "public/", or "the project folder" — for sentences like "served from …". */
export function servedFolderLabel(projectPath: string, docroot: string): string {
  const folder = servedFolder(projectPath, docroot)
  return folder ? `${folder.replace(/\\/g, '/')}/` : 'the project folder'
}
