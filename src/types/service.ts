export type ServiceStatus = 'running' | 'stopped' | 'starting' | 'stopping'

export interface ServiceInfo {
  id: string
  name: string
  category: string
  status: ServiceStatus
  version: string
  /** The port the service binds — for one with several workers, the first
   *  of them; worker `n` binds `port + n`. */
  port: number
  /** Current CPU usage; null while the service is stopped. */
  cpuPercent: number | null
  /** Recent CPU samples driving the sparkline; empty while stopped. */
  cpuHistory: number[]
  /** Set for a service that runs as several identical processes (PHP —
   *  see `services::php_pool`), null for a single-process one. */
  workers: WorkerCount | null
  /** Every port the service binds while running — each worker's, plus a
   *  web UI's. A "port in use" failure can be on any of them. */
  ports: number[]
  /** Whether the service's binary is on disk. One that isn't can't start. */
  installed: boolean
  /** The binary package that installs this service from its own card, or
   *  null where installing is chosen elsewhere (PHP, database versions). */
  installId: string | null
  /** A web UI to open in the browser while the service runs (Mailpit). */
  webUrl: string | null
}

/** How many of a multi-process service's workers are up. */
export interface WorkerCount {
  running: number
  total: number
}

/** Who is listening on a port a service wants. */
export interface PortHolder {
  port: number
  pid: number
  name: string
  path: string | null
  /** `rezure` is a leftover of Rezure's own from a previous run — safe to
   *  reclaim. `system` can't be killed at all. */
  kind: 'rezure' | 'foreign' | 'system'
  /** A ready-to-show sentence naming the holder. */
  description: string
}
