import { Icon } from '../lib/fluent-icon'
import { createFileRoute, Link } from '@tanstack/react-router'
import { useState, type FormEvent } from 'react'
import { AuthFrame } from '../components/AuthFrame'
import { Button } from '../components/ui/button'
import { authClient } from '../lib/auth-client'

export const Route = createFileRoute('/reset-password')({
  validateSearch: (search: Record<string, unknown>) => ({ token: typeof search.token === 'string' ? search.token : '' }),
  component: ResetPasswordPage,
})

function ResetPasswordPage() {
  const { token } = Route.useSearch()
  const [done, setDone] = useState(false)
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setBusy(true); setError('')
    try {
      const form = new FormData(event.currentTarget)
      const result = await authClient.resetPassword({ newPassword: String(form.get('password')), token })
      if (result.error) setError(result.error.message || 'This reset link is invalid or has expired.')
      else setDone(true)
    } catch {
      setError('We could not reach the account service. Please try again.')
    } finally {
      setBusy(false)
    }
  }

  return <AuthFrame eyebrow="ACCOUNT RECOVERY" title={done ? 'Password updated.' : 'Choose a new password.'} subtitle={done ? 'Your password has been changed. You can now sign in.' : 'Use at least 12 characters. Choose a password you do not use elsewhere.'}>
    {!done && <form className="auth-form" onSubmit={submit}><label>New password<input type="password" name="password" autoComplete="new-password" minLength={12} required placeholder="At least 12 characters" /></label>{error && <p className="form-error" role="alert">{error}</p>}<Button type="submit" className="auth-submit" disabled={busy || !token}>{busy ? 'Updating…' : 'Update password'}<Icon icon="fluent:arrow-right-16-regular" width="17" /></Button></form>}
    <p className="auth-foot"><Link to="/login">Back to sign in</Link></p>
  </AuthFrame>
}
