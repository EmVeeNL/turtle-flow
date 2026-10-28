import { Icon } from '../lib/fluent-icon'
import { createFileRoute, Link } from '@tanstack/react-router'
import { useState, type FormEvent } from 'react'
import { AuthFrame } from '../components/AuthFrame'
import { Button } from '../components/ui/button'
import { authClient } from '../lib/auth-client'

export const Route = createFileRoute('/forgot-password')({ component: ForgotPasswordPage })

function ForgotPasswordPage() {
  const [sent, setSent] = useState(false)
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setBusy(true); setError('')
    try {
      const form = new FormData(event.currentTarget)
      const result = await authClient.requestPasswordReset({ email: String(form.get('email')), redirectTo: '/reset-password' })
      if (result.error) setError(result.error.message || 'We could not send the reset email.')
      else setSent(true)
    } catch {
      setError('We could not reach the account service. Please try again.')
    } finally {
      setBusy(false)
    }
  }

  return <AuthFrame eyebrow="ACCOUNT RECOVERY" title={sent ? 'Check your inbox.' : 'Reset your password.'} subtitle={sent ? 'If an account exists for that address, you’ll receive a secure reset link.' : 'Enter the email address you used to create your account.'}>
    {!sent && <form className="auth-form" onSubmit={submit}><label>Email address<input type="email" name="email" autoComplete="email" required placeholder="you@example.com" /></label>{error && <p className="form-error" role="alert">{error}</p>}<Button type="submit" className="auth-submit" disabled={busy}>{busy ? 'Sending…' : 'Send reset link'}<Icon icon="fluent:arrow-right-16-regular" width="17" /></Button></form>}
    <p className="auth-foot"><Link to="/login">Back to sign in</Link></p>
  </AuthFrame>
}
