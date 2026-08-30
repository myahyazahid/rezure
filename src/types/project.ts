export interface ProjectInfo {
  id: string
  name: string
  path: string
  domain: string
  stack: string
  /** Whether `domain` currently resolves to 127.0.0.1 via the OS hosts file. */
  hasHostsEntry: boolean
  /** Unix seconds of the last time this project was opened, or `null` if never. */
  lastOpenedAt: number | null
  openCount: number
}

export interface ProjectTemplate {
  id: string
  name: string
  description: string
  /** What actually builds it — shown as a small tag next to the template. */
  tag: string
}
