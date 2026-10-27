import { Icon } from '../lib/fluent-icon'
import { createFileRoute, redirect } from '@tanstack/react-router'
import { QRCodeSVG } from 'qrcode.react'
import { useEffect, useState, type FormEvent } from 'react'
import { ProtectionGate } from '../components/ProtectionGate'
import { Button } from '../components/ui/button'
import { authClient } from '../lib/auth-client'
import { getAccountProtectionStatus } from '../server/auth.functions'

export const Route = createFileRoute('/security')({
  beforeLoad: async () => {
    const security = await getAccountProtectionStatus()
    if (!security.session) throw redirect({ to: '/login' })
    return { isProtected: security.isProtected }
  },
  component: SecurityPage,
})

function SecurityPage() {
  const { isProtected } = Route.useRouteContext()
  const { data: session } = authClient.useSession()
  const [password, setPassword] = useState('')
  const [code, setCode] = useState('')
  const [setup, setSetup] = useState<{ uri: string; backupCodes: string[] } | null>(null)
  const [message, setMessage] = useState('')
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)
  const [passkeys, setPasskeys] = useState<Array<{ id: string; name: string | null | undefined }>>([])
  const twoFactorEnabled = Boolean(session?.user.twoFactorEnabled)
  const passkeyEnabled = passkeys.length > 0

  async function refreshPasskeys() {
    const result = await authClient.passkey.listUserPasskeys()
    if (result.data) setPasskeys(result.data.map((passkey) => ({ id: passkey.id, name: passkey.name })))
  }

  useEffect(() => { if (session?.user) void refreshPasskeys() }, [session?.user?.id])

  async function beginTotp(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setBusy(true); setError(''); setMessage('')
    const result = await authClient.twoFactor.enable({ password, issuer: 'Turtle Flow' })
    setBusy(false)
    if (result.error) setError(result.error.message || 'Could not start authenticator setup.')
    else if (result.data.method === 'totp') setSetup({ uri: result.data.totpURI, backupCodes: result.data.backupCodes })
  }

  async function confirmTotp(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setBusy(true); setError('')
    const result = await authClient.twoFactor.verifyTotp({ code })
    setBusy(false)
    if (result.error) setError(result.error.message || 'That code did not match.')
    else { setSetup(null); setMessage('Authenticator verification is enabled. Save your recovery codes in a safe place.'); window.location.reload() }
  }

  return <ProtectionGate protectedAccount={isProtected}><main className="main-shell"><header className="topbar"><div className="breadcrumbs"><a href="/">Account</a><Icon icon="fluent:chevron-right-16-regular" width="14" /><strong>Security</strong></div><span className="secure-label"><Icon icon="fluent:shield-lock-16-regular" width="14" /> Security settings</span></header><div className="content-wrap settings-content">
    <div className="eyebrow"><span className="eyebrow-dot" /> ACCOUNT SETTINGS</div><h1>Security<span>.</span></h1><p className="intro settings-intro">Use an authenticator app or a passkey to protect your account. Either one satisfies the security requirement.</p>
    {message && <p className="success-message" role="status"><Icon icon="fluent:checkmark-circle-24-filled" width="18" />{message}</p>}
    {error && <p className="form-error settings-error" role="alert">{error}</p>}
    <section className="setting-card"><div className="setting-card-head"><span className="small-card-icon shield"><Icon icon="fluent:phone-lock-24-regular" width="21" /></span><div><h2>Authenticator app</h2><p>Use a time-based one-time password from your authenticator app.</p></div><span className={`setting-status${twoFactorEnabled ? ' enabled' : ''}`}><i />{twoFactorEnabled ? 'Enabled' : 'Not enabled'}</span></div>
      {twoFactorEnabled ? <div className="setting-card-body"><p>Authenticator verification is active for password sign-ins. Keep your recovery codes somewhere safe.</p><div className="setting-inline"><input type="password" value={password} onChange={(event) => setPassword(event.target.value)} autoComplete="current-password" placeholder="Confirm your password to disable" /><Button variant="secondary" onClick={async () => { setBusy(true); const result = await authClient.twoFactor.disable({ password }); setBusy(false); if (result.error) setError(result.error.message || 'Could not disable authenticator verification.'); else window.location.reload() }} disabled={busy || !password}>Turn off</Button></div></div>
      : setup ? <div className="setting-card-body enroll-grid"><div className="qr-frame"><QRCodeSVG value={setup.uri} size={176} bgColor="#ffffff" fgColor="#111827" level="M" /></div><div><h3>Scan this QR code</h3><p>Scan with your authenticator app, then enter its current six-digit code to verify setup. Keep the recovery codes below somewhere private.</p><form className="setting-inline" onSubmit={confirmTotp}><input className="code-input" inputMode="numeric" autoComplete="one-time-code" value={code} onChange={(event) => setCode(event.target.value)} required placeholder="000000" /><Button disabled={busy || code.length < 6}>{busy ? 'Verifying…' : 'Verify setup'}</Button></form><details className="recovery-details"><summary>Show one-time recovery codes</summary><div className="recovery-list">{setup.backupCodes.map((backupCode) => <code key={backupCode}>{backupCode}</code>)}</div></details></div></div>
      : <div className="setting-card-body"><p>Add a second step to password sign-ins. Your authenticator secret and recovery codes are stored encrypted by the authentication provider.</p><form className="setting-inline" onSubmit={beginTotp}><input type="password" value={password} onChange={(event) => setPassword(event.target.value)} autoComplete="current-password" required placeholder="Confirm your password" /><Button disabled={busy || !password}>{busy ? 'Starting…' : 'Set up authenticator'}</Button></form></div>}
    </section>
    <section className="setting-card"><div className="setting-card-head"><span className="small-card-icon purchase"><Icon icon="fluent:fingerprint-24-regular" width="21" /></span><div><h2>Passkeys</h2><p>Sign in with a device or security key using WebAuthn.</p></div><span className={`setting-status${passkeyEnabled ? ' enabled' : ''}`}><i />{passkeyEnabled ? `${passkeys.length} added` : 'Not added'}</span></div><div className="setting-card-body"><p>Passkeys are phishing-resistant sign-in credentials. An authenticator app or at least one passkey is required to unlock account pages.</p><Button variant="secondary" onClick={async () => { const result = await authClient.passkey.addPasskey({ name: 'My passkey' }); if (result.error) setError(result.error.message || 'Passkey registration was not completed.'); else { setMessage('Passkey added to your account.'); await refreshPasskeys(); window.location.reload() } }}>Add a passkey</Button>{passkeys.length > 0 && <div className="passkey-list">{passkeys.map((passkey) => <div className="passkey-row" key={passkey.id}><span className="device-icon"><Icon icon="fluent:fingerprint-24-regular" width="17" /></span><span>{passkey.name || 'Passkey'}</span><Button variant="ghost" size="sm" onClick={async () => { const result = await authClient.passkey.deletePasskey({ id: passkey.id }); if (result.error) setError(result.error.message || 'Could not remove this passkey.'); else window.location.reload() }}>Remove</Button></div>)}</div>}</div></section>
    <p className="security-footnote"><Icon icon="fluent:info-24-regular" width="15" /> Passkeys provide phishing-resistant sign-in. Authenticator verification is the separate second factor for password-based sign-ins.</p>
  </div></main></ProtectionGate>
}
