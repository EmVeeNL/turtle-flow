import { Icon } from '../lib/fluent-icon'
import { createFileRoute, Link } from '@tanstack/react-router'
import { useState, type FormEvent } from 'react'
import { AuthFrame } from '../components/AuthFrame'
import { authClient } from '../lib/auth-client'
import { Button } from '../components/ui/button'

export const Route = createFileRoute('/signup')({ component: SignupPage })

function SignupPage() {
  const [error, setError] = useState('')
  const [sent, setSent] = useState(false)
  const [busy, setBusy] = useState(false)

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setBusy(true)
    setError('')
    try {
      const form = new FormData(event.currentTarget)
      const result = await authClient.signUp.email({
        name: String(form.get('name')),
        email: String(form.get('email')),
        password: String(form.get('password')),
        callbackURL: '/login',
      })
      if (result.error) setError(result.error.message || 'We could not create your account.')
      else setSent(true)
    } catch {
      setError('We could not reach the account service. Please try again.')
    } finally {
      setBusy(false)
    }
  }

  return <AuthFrame eyebrow="START WITH TURTLE FLOW" title={sent ? 'Check your inbox.' : 'Create your account.'} subtitle={sent ? 'We sent a verification link to your email. Verify it to finish setting up your account.' : 'A secure home for your Turtle Flow license.'}>
    {!sent && <form className="auth-form" onSubmit={submit}>
      <label>Your name<input type="text" name="name" autoComplete="name" required minLength={2} placeholder="How should we address you?" /></label>
      <label>Email address<input type="email" name="email" autoComplete="email" required placeholder="you@example.com" /></label>
      <label>Password<input type="password" name="password" autoComplete="new-password" required minLength={12} placeholder="At least 12 characters" /></label>
      {error && <p className="form-error" role="alert">{error}</p>}
      <Button type="submit" className="auth-submit" disabled={busy}>{busy ? 'Creating account…' : 'Create account'}<Icon icon="fluent:arrow-right-16-regular" width="17" /></Button>
    </form>}
    {sent && <div className="verification-note"><Icon icon="fluent:mail-checkmark-24-regular" width="24" /><span>Check your spam folder if the message doesn’t arrive in a few minutes.</span></div>}
    <p className="auth-foot">Already have an account? <Link to="/login">Sign in</Link></p>
  </AuthFrame>
}
