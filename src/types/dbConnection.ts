import type { DbEngine } from './dbProfile'

/** Which kind of server a connection points at. Older saved connections
 *  have no `kind` and read as `mysql`. */
export type ServerKind = 'mysql' | 'sqlserver' | 'postgres'

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
  kind: ServerKind
  /** The engine a MySQL-family server runs — the client binary is picked
   *  from it. Ignored for every other kind. */
  engine: DbEngine
  /** SQL Server only: sign in as the Windows user, with no password. */
  windowsAuth: boolean
  /** SQL Server only: accept a certificate that can't be verified — what a
   *  self-signed development server needs with ODBC Driver 18. */
  trustServerCertificate: boolean
  /** A server Rezure runs itself — its LocalDB instance or its PostgreSQL:
   *  local, writable, not removable by hand. */
  managed: boolean
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
  /** False when nothing is installed that could talk to it — no MySQL or
   *  MariaDB build, no ODBC Driver for SQL Server, no PostgreSQL build for
   *  `psql`. The switcher disables the row and says so rather than failing
   *  later. */
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
