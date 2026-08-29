export type LogLevel = 'info' | 'warn' | 'error'

export interface LogEntry {
  id: number
  time: string
  service: string
  level: LogLevel
  message: string
}
