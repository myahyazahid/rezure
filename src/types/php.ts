export interface PhpVersion {
  id: string
  version: string
  installed: boolean
  active: boolean
  /** False for versions dropped into the user's own bin folder — Rezure
   *  never checksum-verified those. */
  managed: boolean
  /** Folder this version lives in. */
  path: string
}

/** A version php.net currently publishes for Windows. */
export interface PhpRelease {
  version: string
  branch: string
  downloadUrl: string
  sha256: string
  /** Archive size as php.net reports it, e.g. "33.46MB". */
  size: string
  /** YYYY-MM-DD. */
  released: string
  latest: boolean
  installed: boolean
}

/** Result of switching the active version — the switch also reloads the
 *  running PHP service, and says whether that worked. */
export interface PhpSwitchResult {
  versions: PhpVersion[]
  restarted: boolean
  restartError: string | null
}

/** State of the optional "make Rezure's PHP the system `php`" link. */
export interface PhpPathStatus {
  /** The single directory Rezure puts on PATH. */
  linkDir: string
  onPath: boolean
  /** Where the link currently points. */
  target: string | null
  /** Whether the link matches the active version. */
  inSync: boolean
  /** Other PHP installs already on PATH that enabling would override. */
  conflicts: string[]
}
