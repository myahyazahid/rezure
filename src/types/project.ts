/** How nginx serves a project: its files through PHP, or every request
 *  passed to its dev server (see `services::dev_servers`). */
export type ServeProfile = 'php' | 'proxy'

export interface ProjectInfo {
  id: string
  name: string
  path: string
  domain: string
  /** The framework's label (Laravel, CodeIgniter 4, …), shown as the badge. */
  stack: string
  /** The framework id it's served as — a registry id, `custom` or `unknown`. */
  framework: string
  /** What detection alone says, for "Auto (detected: X)". */
  detectedFramework: string
  /** True when the user picked the framework rather than detection. */
  frameworkOverridden: boolean
  /** The folder nginx serves, absolute — `public\` for Laravel and CodeIgniter 4. */
  docroot: string
  serve: ServeProfile
  /** The port a `proxy` project's dev server listens on, or `null` for a
   *  `php` project (and briefly for a `proxy` one, until a port is picked). */
  devPort: number | null
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
  /** The folder name can't be used as a domain (a space, `;`, `{`…), so it
   *  isn't served — writing it to the nginx config would break every site. */
  domainInvalid: boolean
  /** This project's own pinned PHP version, or `null` to follow the global
   *  active version (Switch page). Pinning a version distinct from the
   *  global default gives this project its own concurrently-running
   *  `php-cgi`, so projects on different versions can run at the same time.
   *  A terminal opened for this project resolves `php` as the same version. */
  phpVersion: string | null
  /** This project's own pinned Node.js version, or `null` to follow the
   *  global active version. What a terminal opened for this project resolves
   *  `node`/`npm`/`npx` as, and what its dev server runs on. */
  nodeVersion: string | null
}

/** What linking a folder would produce, shown before anything is saved. */
export interface LinkPreview {
  path: string
  name: string
  domain: string
  /** The label of the framework it would be served as. */
  stack: string
  /** That framework's id — the user's choice when they made one. */
  framework: string
  detectedFramework: string
  /** The folder nginx would serve — `public/` for Laravel. */
  docroot: string
  /** `proxy`: served through a dev server, not from `docroot`. */
  serve: ServeProfile
  /** True when the domain had to be suffixed to avoid a clash. */
  domainAdjusted: boolean
  /** Set when the picked folder is the docroot *of* a project above it. */
  suggestion: LinkSuggestion | null
}

/** "This is the `public/` folder of a CodeIgniter 4 project — add the project instead?" */
export interface LinkSuggestion {
  path: string
  /** Label of the framework detected in `path`. */
  framework: string
  /** The docroot the picked folder turned out to be (`public`). */
  folder: string
}

/** A framework the pickers can offer. */
export interface FrameworkSummary {
  id: string
  label: string
  /** The subfolder it's served from, or `null` for the project folder itself. */
  docroot: string | null
  serve: ServeProfile
  /** `user` when it comes from the user's own `etc\frameworks.json`. */
  source: 'builtin' | 'user'
  /** The heading it's listed under. The catalog arrives ordered by it, so a
   *  heading starts wherever this changes. */
  category: string
}

export interface FrameworkCatalog {
  frameworks: FrameworkSummary[]
  /** Definitions that were skipped, and saved choices that no longer apply. */
  problems: string[]
}

/** What a project should be served as: a framework by id, a subfolder
 *  ("Custom"), or `null` to go with detection. */
export type FrameworkChoice = { framework: string } | { docroot: string } | null

export interface ProjectTemplate {
  id: string
  name: string
  description: string
  /** What actually builds it — shown as a small tag next to the template. */
  tag: string
}
