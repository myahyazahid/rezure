export interface DatabaseInfo {
  name: string
  collation: string
  tableCount: number
  /** Data + index bytes as reported by `information_schema` — a storage
   *  estimate for InnoDB, not an exact byte count. */
  sizeBytes: number
  /** Domain of the project this database appears to belong to, matched by
   *  name on the Rust side. `null` when nothing matches. */
  usedBy: string | null
}

export interface DatabaseServerInfo {
  host: string
  port: number
  user: string
  hasPassword: boolean
  /** Connection string ready to paste into a client. */
  dsn: string
}

export interface DbClientInfo {
  id: string
  name: string
  /** Whether this client can be opened straight onto one database, or only
   *  onto the server. */
  opensDatabase: boolean
}
