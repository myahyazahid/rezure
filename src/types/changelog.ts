export interface ChangelogEntry {
  version: string
  title: string
  body: string
  releasedAt: string
}

/** A newer major line announced from the dashboard — a link to the website,
 *  not an in-app update. */
export interface UpgradeNotice {
  major: number
  message: string
  url: string
}
