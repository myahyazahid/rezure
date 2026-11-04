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
  /** What the project is built on and needs to run, against what Rezure
   *  has — `null` when its files say nothing. */
  stackInfo: ProjectStack | null
}

/** Read from the project's own files in Rust (`services::project_stack`);
 *  nothing is run to get it. */
export interface ProjectStack {
  frameworks: FrameworkVersion[]
  php: RuntimeRequirement | null
  node: RuntimeRequirement | null
  packageManager: PackageManagerInfo | null
  databases: DatabaseNeed[]
  /** A runtime that doesn't fit, or a database Rezure doesn't have. */
  needsAttention: boolean
}

export interface FrameworkVersion {
  name: string
  /** `11.9.2` when installed, the declared range (`^15.0`) otherwise. */
  version: string | null
  installed: boolean
}

/** One requirement as the project states it. */
export interface StatedRequirement {
  constraint: string
  /** `composer.json`, `Vite 6.4.1`, `.nvmrc (20.11.0)`. */
  source: string
  /** Whether the serving version meets it; `null` when unknown. */
  fits: boolean | null
}

/** What a project asks of PHP or Node.js, against the version serving it. */
export interface RuntimeRequirement {
  /** The one the serving version fails, or the first one read. */
  constraint: string
  source: string
  /** False when nothing could be parsed; nothing below is known then. */
  readable: boolean
  /** Every requirement read, in reading order. */
  stated: StatedRequirement[]
  /** The version serving it now — its pin, or the global active version. */
  serving: string | null
  /** `null` when unknown: nothing installed, or unreadable. */
  fits: boolean | null
  /** Installed versions that fit every requirement, newest first. */
  fitting: string[]
  /** What to pin to, while the serving version doesn't fit. */
  suggested: string | null
  /** The oldest line that fits — `8.2` for PHP, `20` for Node.js. */
  minimumBranch: string | null
}

export interface PackageManagerInfo {
  name: 'npm' | 'pnpm' | 'yarn' | 'bun' | string
  /** From `packageManager` (`pnpm@9.12.0`). */
  version: string | null
  /** `packageManager in package.json`, `pnpm-lock.yaml`, `no lockfile yet`. */
  source: string
  /** `engines.<name>`, when stated. */
  constraint: string | null
  /** npm's version, bundled with the Node.js serving the project. */
  current: string | null
  fits: boolean | null
}

export type DbEngine = 'mysql' | 'mariadb' | 'postgres' | 'sqlserver' | 'sqlite' | 'mongodb'

export interface DatabaseNeed {
  engine: DbEngine
  label: string
  /** `.env (DB_CONNECTION=mysql)`, `prisma/schema.prisma`, `dependency pg`. */
  source: string
  /** A minimum the project states (only WordPress does). */
  minimum: string | null
  /** False for an engine Rezure doesn't offer (MongoDB). */
  supported: boolean
  /** The Rezure service that would serve it; `null` for SQLite (a file)
   *  and for engines Rezure doesn't offer. */
  provider: DbProvider | null
  /** `minimum` against the provider's version; `null` when unknown. */
  fits: boolean | null
}

export interface DbProvider {
  serviceId: string
  /** `MariaDB` or `MySQL` for the same service, per the active data directory. */
  name: string
  version: string | null
  installed: boolean
  running: boolean
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
