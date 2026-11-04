import type { ServerKind } from './dbConnection'

export interface DatabaseInfo {
  name: string
  collation: string
  tableCount: number
  /** Data + index bytes as reported by `information_schema` — a storage
   *  estimate for InnoDB, not an exact byte count. */
  sizeBytes: number
  /** Domain of the project this database appears to belong to, matched by
   *  name on the Rust side. `null` when nothing matches, and always null for
   *  a remote server, where local project folders say nothing about it. */
  usedBy: string | null
}

export interface DatabaseServerInfo {
  host: string
  port: number
  user: string
  hasPassword: boolean
  /** Connection string ready to paste into a client. Never carries the
   *  password, even for a remote connection that has one. */
  dsn: string
  /** True when this is a remote connection rather than the local server. */
  remote: boolean
  /** Name of the active connection or profile, to state beside the endpoint. */
  label: string
  /** True when writes — create, drop, import — are refused for this target. */
  readOnly: boolean
  kind: ServerKind
  /** False where Export can't work — a SQL Server Rezure doesn't run, whose
   *  BACKUP would land on that server's own disk. */
  exportSupported: boolean
  /** True for servers Rezure runs itself — the local MySQL-family one and
   *  Rezure's own PostgreSQL — the only places Delete is offered. Remote
   *  connections and SQL Server are false. */
  dropSupported: boolean
  /** What Import accepts, without the dot: `sql`, plus `bak` on LocalDB. */
  importExtensions: string[]
}

/** Emitted on `database://export-progress` while `exportDatabase` runs. See
 *  `services::database::ExportProgress` on the Rust side. */
export interface ExportProgress {
  name: string
  bytesWritten: number
  /** A `.sql` dump is text written from the schema's raw storage size, so
   *  the two rarely match exactly — this is an estimate to divide by, not a
   *  promise the file will stop growing there. `null` when the size
   *  couldn't be read, in which case there's nothing to show a percentage
   *  against. */
  estimatedTotalBytes: number | null
}

export interface DbClientInfo {
  id: string
  name: string
  /** Whether this client can be opened straight onto one database, or only
   *  onto the server. */
  opensDatabase: boolean
  /** The client's own icon as a `data:` URL, read out of its `.exe`. `null`
   *  for the consoles, which show the server's mark instead. */
  icon: string | null
}

/** A user on Rezure's own PostgreSQL — mirrors
 *  `services::postgres_roles::PostgresRole`. */
export interface PostgresRole {
  name: string
  canLogin: boolean
  canCreateDb: boolean
  superuser: boolean
  /** `postgres` and `root`, which Rezure's server comes with — listed, but
   *  not editable. */
  builtin: boolean
  /** Databases it owns. It can't be deleted while there are any. */
  ownedDatabases: string[]
}

/** A user to create — mirrors `services::postgres_roles::NewRole`. */
export interface NewPostgresRole {
  name: string
  password: string | null
  canCreateDb: boolean
  /** A database to create, owned by the new user. */
  database: string | null
}

/** One way to write an app's database settings — mirrors
 *  `services::postgres_roles::ConnectionSnippet`. */
export interface ConnectionSnippet {
  id: string
  label: string
  text: string
}

/** An installed Windows service — mirrors
 *  `services::windows_services::ServiceState`. */
export interface WindowsServiceState {
  name: string
  displayName: string
  running: boolean
  startsWithWindows: boolean
}

/** Odoo installed on this PC — mirrors `services::odoo::OdooInstall`. */
export interface OdooInstall {
  /** Its folder — also the id the actions take. */
  dir: string
  version: string | null
  confPath: string
  /** Odoo's own Windows service. */
  service: WindowsServiceState | null
  /** The PostgreSQL service Odoo's installer added, if it did. */
  bundledPostgres: WindowsServiceState | null
  /** The PostgreSQL version Odoo's installer bundled, if it's on disk. */
  bundledPostgresVersion: string | null
  dbHost: string | null
  dbPort: string | null
  dbUser: string | null
  pgPath: string | null
  httpPort: number
  /** What connecting sets pg_path to — the active PostgreSQL's bin. */
  rezurePgPath: string | null
  rezurePostgresVersion: string | null
  /** How odoo.conf's `admin_passwd` (Odoo's master password) is set — never
   *  its value. */
  masterPassword: 'missing' | 'plainText' | 'hashed'
  /** Odoo's own icon, read from this install as a data: URL. */
  icon: string | null
  /** This Windows account may start and stop Odoo's service without an
   *  admin prompt. */
  controlAllowed: boolean
  /** Its settings point at Rezure's PostgreSQL, but the login they name
   *  isn't there. */
  loginMissing: boolean
  /** Its settings point at Rezure's PostgreSQL. */
  usesRezure: boolean
  /** The original odoo.conf is kept and can be put back. */
  canRestore: boolean
}
