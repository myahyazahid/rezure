export interface ProjectInfo {
  id: string
  name: string
  path: string
  domain: string
  stack: string
  /** Whether `domain` currently resolves to 127.0.0.1 via the OS hosts file. */
  hasHostsEntry: boolean
}
