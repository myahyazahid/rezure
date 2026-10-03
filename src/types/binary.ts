export interface BinaryStatus {
  id: string
  name: string
  version: string
  installed: boolean
  /** Set for a Microsoft installer (`services::msi`): the license the user
   *  has to accept before Rezure may install it. Null for portable zips. */
  licenseUrl: string | null
}

/** `installing` is a Windows installer running — the stage that waits on the
 *  user's UAC prompt, with no byte count to show. */
export type InstallStage = 'downloading' | 'verifying' | 'extracting' | 'installing' | 'done'

export interface InstallProgress {
  id: string
  stage: InstallStage
  downloadedBytes: number
  totalBytes: number | null
}
