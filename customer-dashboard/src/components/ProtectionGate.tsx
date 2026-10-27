import { useEffect, useState, type FormEvent } from 'react'
import { QRCodeSVG } from 'qrcode.react'
import { authClient } from '../lib/auth-client'
import { Icon } from '../lib/fluent-icon'
import { Button } from './ui/button'

type TotpSetup = { uri: string; backupCodes: string[] }

export function ProtectionGate({ protectedAccount, children }: { protectedAccount: boolean; children: React.ReactNode }) {
  const [method, setMethod] = useState<'totp' | 'passkey' | null>(null)
  const [password, setPassword] = useState('')
  const [code, setCode] = useState('')
  const [totp, setTotp] = useState<TotpSetup | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')

  useEffect(() => {
    if (protectedAccount) return
    const previousOverflow = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    return () => { document.body.style.overflow = previousOverflow }
  }, [protectedAccount])

  async function addPasskey() {
    setMethod('passkey')
    setBusy(true)
    setError('')
    try {
      const result = await authClient.passkey.addPasskey({ name: 'Turtle Flow passkey' })
      if (result.error) setError(result.error.message || 'Passkey setup was not completed.')
      else window.location.reload()
    } catch {
      setError('Passkey setup could not reach the account service. Please try again.')
    } finally {
      setBusy(false)
    }
  }

  async function beginTotp(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setBusy(true)
    setError('')
    try {
      const result = await authClient.twoFactor.enable({ password, issuer: 'Turtle Flow' })
      if (result.error) setError(result.error.message || 'Could not start authenticator setup.')
      else if (result.data.method === 'totp') setTotp({ uri: result.data.totpURI, backupCodes: result.data.backupCodes })
    } catch {
      setError('Authenticator setup could not reach the account service. Please try again.')
    } finally {
      setBusy(false)
    }
  }

  async function confirmTotp(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setBusy(true)
    setError('')
    try {
      const result = await authClient.twoFactor.verifyTotp({ code })
      if (result.error) setError(result.error.message || 'That code did not match.')
      else window.location.reload()
    } catch {
      setError('Verification could not reach the account service. Please try again.')
    } finally {
      setBusy(false)
    }
  }

  return <>
    <div className="protection-locked-content" inert={!protectedAccount} aria-hidden={!protectedAccount}>
      {children}
    </div>
    {!protectedAccount && <div className="protection-overlay">
      <section className="protection-dialog" role="dialog" aria-modal="true" aria-labelledby="protection-title" aria-describedby="protection-description">
        <div className="protection-dialog-mark"><Icon icon="fluent:shield-lock-24-regular" width="25" /></div>
        <div className="eyebrow"><span className="eyebrow-dot" /> ONE QUICK SECURITY STEP</div>
        <h2 id="protection-title">Protect your account<span>.</span></h2>
        <p id="protection-description" className="protection-description">Set up an authenticator app or add a passkey to continue. Either one is enough to unlock your dashboard.</p>

        {!method && <div className="protection-methods">
          <button className="protection-method" onClick={() => setMethod('totp')}>
            <span className="protection-method-icon"><Icon icon="fluent:phone-lock-24-regular" width="21" /></span>
            <span><strong>Authenticator app</strong><small>Use six-digit codes from your authenticator.</small></span>
            <Icon className="protection-method-arrow" icon="fluent:chevron-right-16-regular" width="16" />
          </button>
          <button className="protection-method" onClick={() => void addPasskey()}>
            <span className="protection-method-icon"><Icon icon="fluent:fingerprint-24-regular" width="21" /></span>
            <span><strong>Passkey</strong><small>Use Face ID, Touch ID, Windows Hello, or a security key.</small></span>
            <Icon className="protection-method-arrow" icon="fluent:chevron-right-16-regular" width="16" />
          </button>
        </div>}

        {method === 'totp' && !totp && <form className="protection-step" onSubmit={beginTotp}>
          <label>Confirm your password<input type="password" value={password} onChange={(event) => setPassword(event.target.value)} autoComplete="current-password" required placeholder="Your account password" /></label>
          <Button type="submit" disabled={busy || !password}>{busy ? 'Preparing authenticator…' : 'Continue'}</Button>
        </form>}

        {method === 'totp' && totp && <div className="protection-totp-step">
          <div className="protection-qr"><QRCodeSVG value={totp.uri} size={156} bgColor="#ffffff" fgColor="#111827" level="M" /></div>
          <p>Scan this code with your authenticator app, then enter its current six-digit code.</p>
          <form className="protection-step" onSubmit={confirmTotp}>
            <label>Authenticator code<input inputMode="numeric" autoComplete="one-time-code" value={code} onChange={(event) => setCode(event.target.value)} required placeholder="000000" /></label>
            <Button type="submit" disabled={busy || code.length < 6}>{busy ? 'Verifying…' : 'Verify and continue'}</Button>
          </form>
          <details className="recovery-details"><summary>Show recovery codes</summary><div className="recovery-list">{totp.backupCodes.map((backupCode) => <code key={backupCode}>{backupCode}</code>)}</div></details>
        </div>}

        {method === 'passkey' && <div className="protection-passkey-step"><span className="passkey-prompt-icon"><Icon icon="fluent:fingerprint-24-regular" width="28" /></span><p>{busy ? 'Follow the prompt from your device to create a passkey.' : 'Passkey setup was not completed. You can try again or choose another method.'}</p>{!busy && <Button onClick={() => void addPasskey()}>Try again</Button>}</div>}

        {error && <p className="form-error protection-error" role="alert">{error}</p>}
        {method && <button className="protection-back" onClick={() => { setMethod(null); setTotp(null); setError('') }}>Choose another method</button>}
        <div className="protection-dialog-foot"><Icon icon="fluent:lock-closed-16-filled" width="14" /> Your account stays private while setup is incomplete.</div>
      </section>
    </div>}
  </>
}
