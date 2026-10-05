/** A picture the server described, downloaded and verified by the backend and
 *  handed over as a `data:` URL — exactly as the server sent it (a QRIS has to
 *  stay scannable, so it is never re-encoded or cropped). */
export interface DataImage {
  format: 'png' | 'jpg' | 'webp' | 'svg'
  /** Lowercase hex SHA-256 of the file. */
  sha256: string
  /** `data:image/…;base64,…` — an `<img>` source. */
  dataUrl: string
}

export interface DonateLink {
  /** The donate method's id. `null` only from a server that predates ids. */
  id: number | null
  label: string
  url: string
  /** The link's logo, if the maintainer uploaded one and it could be got. */
  icon: DataImage | null
}

export interface CryptoWallet {
  /** What a wallet is told apart by. The same coin on two networks is two
   *  wallets with the same `symbol`, so never key anything by the symbol. */
  id: number | null
  symbol: string
  /** The blockchain the address lives on (`Tron (TRC-20)`) — sending on the
   *  wrong one usually loses the funds, so it is shown beside the address.
   *  `null` for a wallet saved before the field existed. */
  network: string | null
  label: string
  address: string
  icon: DataImage | null
}

/** What `fetch_donate_config` returns — `services::donate::DonatePage`. */
export interface DonateConfig {
  message: string
  local: DonateLink[]
  global: DonateLink[]
  crypto: CryptoWallet[]
  /** `null` when the server has none, or when there's no picture to show
   *  (see `qrisUnavailable`). */
  qris: DataImage | null
  /** The server has a QRIS, but there's no picture of it to show at all. */
  qrisUnavailable: boolean
  /** The picture shown is the one saved earlier: the current one couldn't be
   *  got, and this one may since have been replaced. */
  qrisStale: boolean
  /** The server couldn't be reached, so this is what it said last time. */
  offline: boolean
}
