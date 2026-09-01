export type ServiceStatus = 'running' | 'stopped' | 'starting' | 'stopping'

export interface ServiceInfo {
  id: string
  name: string
  category: string
  status: ServiceStatus
  version: string
  port: number
  /** Current CPU usage; null while the service is stopped. */
  cpuPercent: number | null
  /** Recent CPU samples driving the sparkline; empty while stopped. */
  cpuHistory: number[]
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
