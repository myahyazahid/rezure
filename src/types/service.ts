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
