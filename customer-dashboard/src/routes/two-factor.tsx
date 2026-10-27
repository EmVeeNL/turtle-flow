import { Icon } from '../lib/fluent-icon'
import { createFileRoute } from '@tanstack/react-router'
import { useState, type FormEvent } from 'react'
import { authClient } from '../lib/auth-client'
import { AuthFrame } from '../components/AuthFrame'
import { Button } from '../components/ui/button'

export const Route = createFileRoute('/two-factor')({ validateSearch: (search: Record<string, unknown>) => ({ callbackURL: typeof search.callbackURL === 'string' ? search.callbackURL : '/' }), component: TwoFactorPage })

function TwoFactorPage() {
  const { callbackURL } = Route.useSearch()
  const destination = callbackURL.startsWith('/') && !callbackURL.startsWith('//') ? callbackURL : '/'
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)
  const [recovery, setRecovery] = useState(false)

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setBusy(true)
    setError('')
    const form = new FormData(event.currentTarget)
    const code = String(form.get('code')).trim()
    const result = recovery
      ? await authClient.twoFactor.verifyBackupCode({ code })
      : await authClient.twoFactor.verifyTotp({ code })
    setBusy(false)
    if (result.error) setError(result.error.message || 'That code could not be verified.')
    else window.location.assign(destination)
  }

  return <AuthFrame eyebrow="TWO-STEP VERIFICATION" title="One more step." subtitle={recovery ? 'Enter one of your unused recovery codes.' : 'Enter the six-digit code from your authenticator app.'}>
    <form className="auth-form" onSubmit={submit}>
      <label>{recovery ? 'Recovery code' : 'Authentication code'}<input className="code-input" name="code" inputMode={recovery ? 'text' : 'numeric'} autoComplete="one-time-code" required autoFocus placeholder={recovery ? 'XXXX-XXXX-XXXX' : '000000'} /></label>
      {error && <p className="form-error" role="alert">{error}</p>}
      <Button className="auth-submit" disabled={busy}>{busy ? 'Verifying…' : 'Verify and continue'}<Icon icon="fluent:arrow-right-16-regular" width="17" /></Button>
    </form>
    <button className="text-button recovery-switch" onClick={() => { setRecovery(!recovery); setError('') }}>{recovery ? 'Use authenticator code' : 'Use a recovery code'}</button>
  </AuthFrame>
}
