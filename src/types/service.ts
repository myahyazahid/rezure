export type ServiceStatus = 'running' | 'stopped' | 'starting' | 'stopping'

export interface ServiceInfo {
  id: string
  name: string
  category: string
  status: ServiceStatus
  version: string
  port: number
}
