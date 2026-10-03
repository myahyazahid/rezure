export interface DonateLink {
  label: string
  url: string
}

export interface CryptoWallet {
  symbol: string
  label: string
  address: string
}

export interface DonateConfig {
  message: string
  local: DonateLink[]
  global: DonateLink[]
  crypto: CryptoWallet[]
}
