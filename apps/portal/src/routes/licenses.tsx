import { useEffect, useState } from 'react'
import { Icon } from '../lib/fluent-icon'
import { createFileRoute, redirect } from '@tanstack/react-router'
import { ProtectionGate } from '../components/ProtectionGate'
import { getAccountProtectionStatus } from '../server/auth.functions'
import { createMyFreeLicense, getMyLicenses } from '../server/licenses.functions'
import { formatDate } from '../lib/dates'

export const Route = createFileRoute('/licenses')({
  beforeLoad: async () => {
    const security = await getAccountProtectionStatus()
    if (!security.session) throw redirect({ to: '/login' })
    return {
      isProtected: security.isProtected,
      licenses: security.isProtected ? await getMyLicenses() : [],
    }
  },
  component: LicensesPage,
})

function LicensesPage() {
  const { licenses, isProtected } = Route.useRouteContext()
  const [creating, setCreating] = useState(false)
  const [error, setError] = useState('')
  const [installations, setInstallations] = useState<Array<{ id: string; name: string; platform: string; version: string; lastAuthorizedAt: string; revokedAt: string | null }>>([])
  const [deviceError, setDeviceError] = useState('')

  useEffect(() => {
    if (!isProtected) return
    void fetch('/api/cli/installations').then(async (response) => {
      if (response.ok) setInstallations(await response.json() as typeof installations)
    }).catch(() => setDeviceError('Linked CLI devices could not be loaded. Refresh to try again.'))
  }, [isProtected])

  async function revokeInstallation(id: string) {
    setDeviceError('')
    try {
      const response = await fetch(`/api/cli/installations/${encodeURIComponent(id)}`, { method: 'DELETE' })
      if (!response.ok) throw new Error('This device could not be revoked. Please try again.')
      setInstallations((items) => items.map((item) => item.id === id ? { ...item, revokedAt: new Date().toISOString() } : item))
    } catch (cause) { setDeviceError(cause instanceof Error ? cause.message : 'This device could not be revoked.') }
  }

  async function createFreeLicense() {
    setCreating(true)
    setError('')
    try {
      const result = await createMyFreeLicense()
      if (!result.licenseId) throw new Error('Could not create your license. Please try again.')
      window.location.reload()
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'Could not create your license. Please try again.')
      setCreating(false)
    }
  }

  return <ProtectionGate protectedAccount={isProtected}>
    <main className="main-shell">
      <header className="topbar">
        <div className="breadcrumbs"><a href="/">Account</a><Icon icon="fluent:chevron-right-16-regular" width="14" /><strong>Licenses</strong></div>
        <span className="secure-label"><Icon icon="fluent:key-24-regular" width="14" /> License ledger</span>
      </header>
      <div className="content-wrap settings-content">
        <div className="eyebrow"><span className="eyebrow-dot" /> YOUR ENTITLEMENTS</div>
        <h1>Licenses<span>.</span></h1>
        <p className="intro settings-intro">Create and manage your Turtle Flow license.</p>
        {licenses.length === 0 ? <section className="empty-state">
          <span className="empty-icon"><Icon icon="fluent:key-24-regular" width="27" /></span>
          <h2>Your free license is ready</h2>
          <p>Every account can create one free license for local use. It never expires and includes a signed license file for download.</p>
          {error && <p className="form-error settings-error" role="alert">{error}</p>}
          <button className="button button-outline" onClick={createFreeLicense} disabled={creating || !isProtected}>
            <Icon icon="fluent:key-24-regular" width="16" /> {creating ? 'Creating license…' : 'Create free license'}
          </button>
          <span className="coming-soon"><span /> ONE LICENSE PER ACCOUNT</span>
        </section> : licenses.map((item) => <section className="license-ledger-card" key={item.id}>
          <div className="ledger-head">
            <div className="ledger-icon"><Icon icon="fluent:leaf-three-24-regular" width="22" /></div>
            <div><span className="card-kicker">TURTLE FLOW LICENSE</span><h2>License <code>••••{item.id.slice(-4)}</code></h2></div>
            <span className={`setting-status${item.status === 'active' ? ' enabled' : ''}`}><i />{item.status}</span>
          </div>
          <div className="ledger-meta">
            <span><small>ISSUED</small>{formatDate(item.issuedAt)}</span>
            <span><small>VALID THROUGH</small>{item.expiresAt ? formatDate(item.expiresAt) : 'No expiry date'}</span>
            <span><small>SIGNING KEY</small>{item.keyId}</span>
          </div>
          <div className="ledger-actions">
            <span>Signed free license · local use · never expires</span>
            <a className="button button-outline" href={`/api/licenses/${encodeURIComponent(item.id)}/download`}><Icon icon="fluent:arrow-download-24-regular" width="16" /> Download license</a>
          </div>
        </section>)}
        <section className="linked-devices"><div className="linked-devices-heading"><div><span className="card-kicker">LOCAL CLI ACCESS</span><h2>Linked CLI installations</h2></div><a className="button button-outline" href="/device">Link a CLI</a></div><p>Up to two CLI installations can be linked. Project data stays on each device.</p>
          {deviceError && <p className="form-error" role="alert">{deviceError}</p>}
          {installations.length === 0 ? <span className="coming-soon"><span /> NO DEVICES LINKED</span> : <div className="linked-device-list">{installations.map((item) => <article className="linked-device-row" key={item.id}><div className="ledger-icon"><Icon icon="fluent:desktop-24-regular" width="21" /></div><div className="linked-device-info"><strong>{item.name}</strong><span>{item.platform} · CLI {item.version} · linked {formatDate(new Date(item.lastAuthorizedAt))}</span></div>{item.revokedAt ? <span className="device-revoked">Revoked</span> : <button className="text-button" onClick={() => void revokeInstallation(item.id)}>Revoke</button>}</article>)}</div>}
        </section>
      </div>
    </main>
  </ProtectionGate>
}
