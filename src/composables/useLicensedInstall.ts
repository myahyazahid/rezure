import { ref } from 'vue'
import { useBinariesStore } from '@/stores/binaries'
import type { BinaryStatus } from '@/types/binary'

function errorMessage(e: unknown): string {
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  return 'Something went wrong.'
}

/**
 * Installs a package, stopping first to ask for license consent when it's
 * one of the Microsoft installers (`services::msi`).
 *
 * Every Install button that can reach the ODBC Driver or LocalDB goes
 * through this, so there is exactly one place where `acceptLicense` becomes
 * true: `confirm()`, which only `LicenseConsentModal` calls, after its box
 * was ticked. A portable zip installs straight away, as before.
 */
export function useLicensedInstall() {
  const binaries = useBinariesStore()
  /** The package waiting on the user's consent; renders the modal. */
  const pending = ref<BinaryStatus | null>(null)
  const error = ref<string | null>(null)

  async function run(id: string, acceptLicense: boolean) {
    error.value = null
    try {
      await binaries.install(id, acceptLicense)
      return true
    } catch (e) {
      error.value = errorMessage(e)
      return false
    }
  }

  async function request(id: string) {
    error.value = null
    if (binaries.binaries.length === 0) await binaries.fetchAll().catch(() => {})
    const pkg = binaries.binaries.find((b) => b.id === id) ?? null
    if (pkg?.licenseUrl) {
      pending.value = pkg
      return false
    }
    return run(id, false)
  }

  async function confirm() {
    const pkg = pending.value
    pending.value = null
    if (!pkg) return false
    return run(pkg.id, true)
  }

  function cancel() {
    pending.value = null
  }

  return { pending, error, request, confirm, cancel }
}
