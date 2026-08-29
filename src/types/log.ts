export type LogLevel = 'info' | 'warn' | 'error'

export interface LogEntry {
  id: number
  time: string
  service: string
  level: LogLevel
  message: string
}

/** Payload of the Rust-side `service://log` event, one per output line. */
export interface ServiceLogEvent {
  serviceId: string
  stream: 'stdout' | 'stderr'
  line: string
}
