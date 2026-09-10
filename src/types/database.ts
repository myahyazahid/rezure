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
}

export interface DbClientInfo {
  id: string
  name: string
  /** Whether this client can be opened straight onto one database, or only
   *  onto the server. */
  opensDatabase: boolean
}
