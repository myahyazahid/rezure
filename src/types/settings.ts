export interface Settings {
  defaultPort: number
  shareUsageData: boolean
  activePhpVersion: string | null
}

export interface SettingsPatch {
  defaultPort?: number
  shareUsageData?: boolean
}

/** Where Rezure's own state lives on disk — read-only, shown so it can be
 *  found without digging through docs. */
export interface StoragePaths {
  wwwRoot: string
  binariesDir: string
  dropInDir: string
  dumpsDir: string
}
