/** A version MariaDB currently publishes for Windows, across every branch
 *  `services::mariadb_catalog::BRANCHES` asks about. */
export interface MariaDbRelease {
  version: string
  branch: string
  downloadUrl: string
  sha256: string
  /** YYYY-MM-DD. */
  released: string
  latest: boolean
  installed: boolean
}

/** A version Composer currently publishes as stable. */
export interface ComposerRelease {
  version: string
  downloadUrl: string
  latest: boolean
  installed: boolean
}

/** One Composer version on disk, and whether it's the active one. */
export interface ComposerVersion {
  id: string
  version: string
  installed: boolean
  active: boolean
}

/** One runtime version found on disk — mirrors
 *  `services::binaries::InstalledVersionStatus`, shared by every runtime
 *  that has no "active version" concept of its own (MariaDB's
 *  installed-versions listing on the Switch page — each database profile
 *  picks its own compatible build, so there's no single active one at this
 *  level). */
export interface InstalledVersion {
  version: string
  path: string
  /** False for anything under the drop-in folder — Rezure never
   *  checksum-verified those. */
  managed: boolean
}

/** One Node.js version on disk, and whether it's the globally active one —
 *  mirrors `services::node::NodeVersionStatus`. Same shape as
 *  {@link ComposerVersion}, but Node additionally supports a per-project
 *  override; see `Project.nodeVersion`. */
export interface NodeVersion {
  id: string
  version: string
  installed: boolean
  active: boolean
  /** False for anything under the drop-in folder — Rezure never
   *  checksum-verified those. */
  managed: boolean
  path: string
}

/** A version nodejs.org currently publishes for Windows x64 — the newest
 *  patch of each LTS line, plus the newest Current release. */
export interface NodeRelease {
  version: string
  /** The LTS codename ("Krypton"), or null for a Current release. */
  lts: string | null
  /** YYYY-MM-DD. */
  released: string
  latest: boolean
  installed: boolean
}
