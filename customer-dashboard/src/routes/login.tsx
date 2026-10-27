import { Icon } from '../lib/fluent-icon'
import { createFileRoute, Link } from '@tanstack/react-router'
import { useState, type FormEvent } from 'react'
import { authClient } from '../lib/auth-client'
import { AuthFrame } from '../components/AuthFrame'
import { Button } from '../components/ui/button'

export const Route = createFileRoute('/login')({ validateSearch: (search: Record<string, unknown>): { callbackURL?: string } => ({ callbackURL: typeof search.callbackURL === 'string' ? search.callbackURL : undefined }), component: LoginPage })

function LoginPage() {
  const { callbackURL } = Route.useSearch()
  const destination = callbackURL?.startsWith('/') && !callbackURL.startsWith('//') ? callbackURL : '/'
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setBusy(true)
    setError('')
    try {
      const form = new FormData(event.currentTarget)
      const result = await authClient.signIn.email({
        email: String(form.get('email')),
        password: String(form.get('password')),
        callbackURL: destination,
      })
      if (result.error) setError(result.error.message || 'Sign in failed. Check your details and try again.')
      else if (!(result.data as { twoFactorRedirect?: boolean } | undefined)?.twoFactorRedirect) window.location.assign(destination)
    } catch {
      setError('We could not reach the account service. Please try again.')
    } finally {
      setBusy(false)
    }
  }

  async function signInWithPasskey() {
    setBusy(true)
    setError('')
    try {
      const result = await authClient.signIn.passkey()
      if (result.error) setError(result.error.message || 'Passkey sign in failed.')
      else window.location.assign(destination)
    } catch {
      setError('We could not reach the account service. Please try again.')
    } finally {
      setBusy(false)
    }
  }

  return <AuthFrame eyebrow="YOUR ACCOUNT, SECURED" title="Welcome back." subtitle="Sign in to manage your Turtle Flow account.">
    <form className="auth-form" onSubmit={submit}>
      <label>Email address<input type="email" name="email" autoComplete="email" required placeholder="you@example.com" /></label>
      <label>Password<input type="password" name="password" autoComplete="current-password" required placeholder="Your password" /></label>
      <Link className="forgot-link" to="/forgot-password">Forgot password?</Link>
      {error && <p className="form-error" role="alert">{error}</p>}
      <Button type="submit" className="auth-submit" disabled={busy}>{busy ? 'Signing in…' : 'Sign in'}<Icon icon="fluent:arrow-right-16-regular" width="17" /></Button>
    </form>
    <div className="auth-divider"><span />or<span /></div>
    <button className="passkey-button" onClick={signInWithPasskey} disabled={busy}><Icon icon="fluent:fingerprint-24-regular" width="19" /> Continue with a passkey</button>
    <p className="auth-foot">New to Turtle Flow? <Link to="/signup">Create an account</Link></p>
  </AuthFrame>
}
