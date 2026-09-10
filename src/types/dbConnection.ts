import type { DbEngine } from './dbProfile'

/** How hard to insist on TLS. `preferred` is what the clients do anyway. */
export type TlsMode = 'disabled' | 'preferred' | 'required'

/** How the SSH session authenticates. A key is the better answer, but
 *  plenty of small VPSes only offer password login. */
export type SshAuth = { kind: 'key'; path: string } | { kind: 'password' }

/** How to reach a database that only listens on its own machine. */
export interface SshTunnel {
  host: string
  port: number
  user: string
  auth: SshAuth
}

export interface DbConnection {
  id: string
  name: string
  host: string
  port: number
  user: string
  /** The engine the *server* runs — the client binary is picked from it. */
  engine: DbEngine
  tlsMode: TlsMode
  /** Refuses create, drop and import. Defaults on for every new connection. */
  readOnly: boolean
  /** Whether the password lives in Windows Credential Manager. */
  savePassword: boolean
  /** When set, `host`/`port` above are resolved on the SSH server, not on
   *  this machine — so they're usually 127.0.0.1 and the real DB port. */
  ssh: SshTunnel | null
  lastUsedAt: number | null
}

/** A connection plus what the backend resolved about it right now. */
export interface DbConnectionStatus extends DbConnection {
  active: boolean
  /** False when no client binary is installed that could talk to it — the
   *  switcher disables the row and says so rather than failing later. */
  clientAvailable: boolean
  /** Whether a password is known: saved, or entered earlier this session.
   *  When false the connection needs unlocking before it can be used. */
  hasPassword: boolean
}

/** What selecting a target left the page looking at. */
export interface TargetResult {
  connections: DbConnectionStatus[]
  label: string
  remote: boolean
}

export const TLS_LABEL: Record<TlsMode, string> = {
  disabled: 'No TLS',
  preferred: 'TLS if offered',
  required: 'TLS required',
}
