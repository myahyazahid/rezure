export interface BinaryStatus {
  id: string
  name: string
  version: string
  installed: boolean
}

export type InstallStage = 'downloading' | 'verifying' | 'extracting' | 'done'

export interface InstallProgress {
  id: string
  stage: InstallStage
  downloadedBytes: number
  totalBytes: number | null
}
