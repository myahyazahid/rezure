import type { ServiceStatus } from '@/types/service'

/** How a dev server is told its port — mirrors `frameworks::PortMode`. */
export type PortMode = 'vite' | 'flag' | 'env' | 'fixed'

export type PackageManager = 'npm' | 'pnpm' | 'yarn' | 'bun'

/** Why a dev server can't be started yet — mirrors
 *  `services::dev_servers::DevServerIssue`, in the order to fix them. */
export type DevServerIssue =
  | { kind: 'noPackageJson' }
  | { kind: 'nodeMissing'; version: string | null }
  | { kind: 'packageManagerMissing'; tool: string }
  | { kind: 'scriptMissing'; script: string | null }
  | { kind: 'needsInstall' }
  | { kind: 'alreadyRunning'; servers: OutsideServer[] }

/** A copy of the project's dev server started outside Rezure (a terminal,
 *  an IDE) — mirrors `dev_servers::OutsideServer`. */
export interface OutsideServer {
  /** What stopping it kills: the topmost node/bun/deno of it, never the
   *  terminal it was typed in. */
  pid: number
  name: string
  ports: number[]
}

/** One row of the Dev servers dialog — mirrors `dev_servers::DevServerRow`. */
export interface DevServerRow {
  projectId: string
  serviceId: string
  name: string
  domain: string
  framework: string
  port: number
  /** What's used: the choice below, or the framework's default. */
  portMode: PortMode
  /** The user's choice; `null` means the framework's. */
  portModeSetting: PortMode | null
  defaultPortMode: PortMode
  /** The script that runs, or `null` when `package.json` has none to run. */
  script: string | null
  /** The user's choice; `null` means the framework's. */
  scriptSetting: string | null
  /** Every script in `package.json` that can be chosen. */
  scripts: string[]
  packageManager: PackageManager
  /** What Start runs, for display. */
  command: string | null
  /** Whether Open starts it when it isn't running. */
  autoStart: boolean
  status: ServiceStatus
  issue: DevServerIssue | null
}

/** A change to one project's dev server settings — mirrors
 *  `dev_servers::DevServerChange`. */
export interface DevServerChange {
  port: number
  portMode: PortMode | null
  script: string | null
  autoStart: boolean
}
