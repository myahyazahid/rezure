export type ServiceStatus = 'running' | 'stopped' | 'starting' | 'stopping' | 'error'

export interface ServiceInfo {
  id: string
  name: string
  status: ServiceStatus
  version: string
  port: number
}
