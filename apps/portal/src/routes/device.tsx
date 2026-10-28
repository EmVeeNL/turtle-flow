import { useState, type FormEvent } from 'react'
import { createFileRoute, redirect } from '@tanstack/react-router'
import { Icon } from '../lib/fluent-icon'
import { ProtectionGate } from '../components/ProtectionGate'
import { getAccountProtectionStatus } from '../server/auth.functions'

export const Route = createFileRoute('/device')({
  beforeLoad: async () => {
    const status = await getAccountProtectionStatus()
    if (!status.session) throw redirect({ to: '/login', search: { callbackURL: '/device' } })
    return { isProtected: status.isProtected }
  },
  component: DevicePage,
})

type DeviceInfo = { name: string; platform: string; version: string }
function DevicePage() {
  const { isProtected } = Route.useRouteContext()
  const [code, setCode] = useState('')
  const [device, setDevice] = useState<DeviceInfo | null>(null)
  const [busy, setBusy] = useState(false)
  const [message, setMessage] = useState('')
  const [error, setError] = useState('')

  async function lookup(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setBusy(true); setError(''); setMessage('')
    try {
      const response = await fetch('/api/cli/device/authorizations/pending', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ user_code: code }) })
      const result = await response.json() as { device?: DeviceInfo; error?: string }
      if (!response.ok || !result.device) throw new Error(result.error === 'invalid_or_expired_code' ? 'That code is invalid or has expired.' : 'Could not find this CLI authorization.')
      setDevice(result.device)
    } catch (cause) { setError(cause instanceof Error ? cause.message : 'Could not find this CLI authorization.') }
    finally { setBusy(false) }
  }

  async function decide(action: 'approve' | 'deny') {
    setBusy(true); setError('')
    try {
      const response = await fetch(`/api/cli/device/authorizations/${action}`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ user_code: code }) })
      const result = await response.json() as { error?: string }
      if (!response.ok) throw new Error(result.error === 'installation_limit' ? 'Both CLI device slots are in use. Revoke a device from your license page and try again.' : 'This authorization could not be completed. It may have expired.')
      setMessage(action === 'approve' ? 'CLI linked. Return to the terminal to finish signing in.' : 'CLI authorization denied.')
      setDevice(null)
    } catch (cause) { setError(cause instanceof Error ? cause.message : 'This authorization could not be completed.') }
    finally { setBusy(false) }
  }

  return <ProtectionGate protectedAccount={isProtected}><main className="main-shell"><header className="topbar"><div className="breadcrumbs"><a href="/">Account</a><Icon icon="fluent:chevron-right-16-regular" width="14" /><strong>Link a CLI</strong></div></header><div className="content-wrap settings-content"><div className="eyebrow"><span className="eyebrow-dot" /> TURTLE FLOW CLI</div><h1>Link this device<span>.</span></h1><p className="intro settings-intro">Enter the one-time code shown by <code>turtle login</code>. Review the device before approving.</p>
    {!device && <form className="auth-form device-code-form" onSubmit={lookup}><label>One-time code<input value={code} onChange={(e) => setCode(e.target.value.toUpperCase())} autoComplete="off" required placeholder="ABCD-EFGH-JKLM" /></label>{error && <p className="form-error" role="alert">{error}</p>}<button className="button button-outline" disabled={busy || !isProtected}>{busy ? 'Checking…' : 'Continue'}</button></form>}
    {device && <section className="license-ledger-card"><div className="ledger-head"><div className="ledger-icon"><Icon icon="fluent:desktop-24-regular" width="22" /></div><div><span className="card-kicker">DEVICE REQUEST</span><h2>{device.name}</h2></div></div><div className="ledger-meta"><span><small>PLATFORM</small>{device.platform}</span><span><small>CLI VERSION</small>{device.version}</span><span><small>ACCOUNT PROTECTION</small>Verified</span></div><div className="ledger-actions"><span>Approval links this CLI and creates your one free local license if needed. Project data stays on your computer.</span><div className="device-actions"><button className="button button-outline" disabled={busy} onClick={() => void decide('deny')}>Deny</button><button className="button button-primary" disabled={busy} onClick={() => void decide('approve')}>{busy ? 'Working…' : 'Approve device'}</button></div></div></section>}
    {message && <p className="device-success" role="status">{message}</p>}
  </div></main></ProtectionGate>
}
