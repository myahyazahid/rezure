export type DbEngine = 'mysql' | 'mariadb'
export type DbProfileSource = 'rezure' | 'laragon' | 'xampp' | 'custom'

export interface DbProfile {
  id: string
  name: string
  datadirPath: string
  engine: DbEngine
  version: string
  port: number
  source: DbProfileSource
  /** A specific server build this profile runs on, when adopting another
   *  tool's install rather than one Rezure manages. */
  binaryDir: string | null
  /** The my.ini this datadir depends on — an adopted install is only
   *  readable under the config it was created with. */
  defaultsFile: string | null
  isDefault: boolean
  lastUsedAt: number | null
}

/** A profile plus what the backend resolved about it right now. */
export interface DbProfileStatus extends DbProfile {
  active: boolean
  /** False when no compatible engine binary can be found — the switcher
   *  disables the row and explains rather than letting the switch fail. */
  binaryAvailable: boolean
}

/** A datadir found on the machine that isn't a profile yet. */
export interface DetectedDatadir {
  name: string
  datadirPath: string
  engine: DbEngine
  version: string
  source: DbProfileSource
  binaryDir: string | null
  defaultsFile: string | null
}

export interface SwitchResult {
  profiles: DbProfileStatus[]
  restarted: boolean
}

export const ENGINE_LABEL: Record<DbEngine, string> = {
  mysql: 'MySQL',
  mariadb: 'MariaDB',
}

export const SOURCE_LABEL: Record<DbProfileSource, string> = {
  rezure: 'Rezure',
  laragon: 'Laragon',
  xampp: 'XAMPP',
  custom: 'Custom',
}
