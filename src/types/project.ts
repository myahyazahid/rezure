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
  /** `scanned` lives in the www folder; `linked` is a folder elsewhere the
   *  user pointed Rezure at, and is the only kind that can be unlinked. */
  kind: 'scanned' | 'linked'
  /** A linked project whose folder is no longer there. Still listed — the
   *  drive may just be unplugged. */
  missing: boolean
}

/** What linking a folder would produce, shown before anything is saved. */
export interface LinkPreview {
  path: string
  name: string
  domain: string
  stack: string
  /** The folder nginx would serve — `public/` for Laravel. */
  docroot: string
  /** True when the domain had to be suffixed to avoid a clash. */
  domainAdjusted: boolean
}

export interface ProjectTemplate {
  id: string
  name: string
  description: string
  /** What actually builds it — shown as a small tag next to the template. */
  tag: string
}
