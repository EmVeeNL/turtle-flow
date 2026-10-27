import { Icon } from '../lib/fluent-icon'
import { createFileRoute, redirect } from '@tanstack/react-router'
import { ProtectionGate } from '../components/ProtectionGate'
import { getAccountProtectionStatus } from '../server/auth.functions'
import { getMyLicenseActivity } from '../server/activity.functions'
import { getMyLicenses } from '../server/licenses.functions'
import { formatDate, formatDateTime } from '../lib/dates'

export const Route = createFileRoute('/')({
  beforeLoad: async () => {
    const security = await getAccountProtectionStatus()
    if (!security.session) throw redirect({ to: '/login' })
    if (!security.isProtected) return { user: security.session.user, isProtected: false, licenses: [], activity: [] }
    const [licenses, activity] = await Promise.all([getMyLicenses(), getMyLicenseActivity()])
    return { user: security.session.user, isProtected: true, licenses, activity }
  },
  component: AccountHome,
})

function AccountHome() {
  const { user, isProtected, licenses, activity } = Route.useRouteContext()
  const firstName = user.name.trim().split(/\s+/)[0] || 'there'
  const latestLicense = licenses[0]

  return (
    <ProtectionGate protectedAccount={isProtected}><main className="main-shell">
      <header className="topbar">
        <div className="breadcrumbs"><span>Account</span><Icon icon="fluent:chevron-right-16-regular" width="14" /><strong>Overview</strong></div>
        <div className="top-actions"><span className="secure-label"><Icon icon="fluent:lock-closed-16-filled" width="14" /> Protected account</span><button className="icon-button notifications" aria-label="Notifications"><Icon icon="fluent:alert-24-regular" width="20" /></button></div>
      </header>

      <div className="content-wrap">
        <section className="welcome-row">
          <div><div className="eyebrow"><span className="eyebrow-dot" /> YOUR TURTLE FLOW ACCOUNT</div><h1>Welcome back, {firstName}<span>.</span></h1><p className="intro">Your license, account security, and activity in one place.</p></div>
          <a className="button button-quiet" href="/security"><Icon icon="fluent:shield-lock-24-regular" width="17" /> Security settings</a>
        </section>

        <section className={`license-card${latestLicense ? '' : ' license-empty'}`} aria-labelledby="license-title">
          <div className="license-glow" />
          {latestLicense ? <>
            <div className="license-topline"><div className="license-overline"><Icon icon="fluent:key-24-regular" width="17" /> LATEST LICENSE</div><span className="status-pill preview-pill">{latestLicense.status}</span></div>
            <div className="license-body"><div><div className="plan-name" id="license-title">Turtle Flow <span>License</span></div><p className="plan-copy">Issued {formatDate(latestLicense.issuedAt)} · {latestLicense.expiresAt ? `Expires ${formatDate(latestLicense.expiresAt)}` : 'No expiry date'}</p></div><div className="license-mark"><Icon icon="fluent:leaf-three-24-regular" width="34" /></div></div>
            <div className="license-meta"><div><span className="meta-label">LICENSE ID</span><span className="meta-value mono">••••{latestLicense.id.slice(-4)}</span></div><div><span className="meta-label">ISSUED</span><span className="meta-value">{formatDate(latestLicense.issuedAt)}</span></div><div><span className="meta-label">VALID THROUGH</span><span className="meta-value">{latestLicense.expiresAt ? formatDate(latestLicense.expiresAt) : 'No expiry date'}</span></div></div>
            <div className="license-footer"><div className="device-stack"><span className="device-icon"><Icon icon="fluent:document-checkmark-24-regular" width="17" /></span><span><strong>{licenses.length} license{licenses.length === 1 ? '' : 's'} in your account</strong><small>Signed for offline verification.</small></span></div><a className="button button-outline" href="/licenses">Manage license <Icon icon="fluent:arrow-right-16-regular" width="16" /></a></div>
          </> : <>
            <div className="license-topline"><div className="license-overline"><Icon icon="fluent:key-24-regular" width="17" /> LICENSE LEDGER</div><span className="status-pill preview-pill">No licenses</span></div>
            <div className="license-body"><div><div className="plan-name" id="license-title">Your license <span>lives here.</span></div><p className="plan-copy">When your account has a license, its status and devices will appear here.</p></div><div className="license-mark"><Icon icon="fluent:leaf-three-24-regular" width="34" /></div></div>
            <div className="license-footer"><div className="device-stack"><span className="device-icon"><Icon icon="fluent:info-24-regular" width="17" /></span><span><strong>Your free license is ready</strong><small>Create one free local license for this account.</small></span></div><a className="button button-outline" href="/licenses">Create free license <Icon icon="fluent:arrow-right-16-regular" width="16" /></a></div>
          </>}
        </section>

        <div className="section-heading"><div><h2>Account activity</h2><p>A record of important changes to your account.</p></div><a className="text-button" href="/licenses">View licenses <Icon icon="fluent:arrow-right-16-regular" width="15" /></a></div>
        <section className="activity-card" aria-label="Recent account activity">
          <div className="activity-head"><span>EVENT</span><span>DATE</span><span>STATUS</span></div>
          {activity.length ? activity.map((item) => <div className="activity-row" key={item.id}><div className="activity-event"><span className="event-icon blue"><Icon icon="fluent:history-24-regular" width="19" /></span><span><strong>{item.eventType.replaceAll('_', ' ')}</strong><small>License account event</small></span></div><time>{formatDateTime(item.createdAt)}</time><span className="event-status"><i />Recorded</span></div>) : <div className="activity-empty"><Icon icon="fluent:history-24-regular" width="21" /><span>No license activity yet</span></div>}
          <div className="activity-bottom"><span><Icon icon="fluent:info-24-regular" width="16" /> Account activity is private and scoped to your licenses.</span><a href="/security">Security settings <Icon icon="fluent:arrow-up-right-16-regular" width="14" /></a></div>
        </section>

        <section className="lower-grid">
          <article className="small-card security-card"><div className="small-card-icon shield"><Icon icon="fluent:shield-lock-24-regular" width="21" /></div><div className="small-card-copy"><span className="card-kicker">ACCOUNT SECURITY</span><h3>Manage sign-in protection</h3><p>Authenticator verification and passkeys protect access to your account.</p><a href="/security">Security settings <Icon icon="fluent:arrow-right-16-regular" width="14" /></a></div></article>
          <article className="small-card purchase-card"><div className="small-card-icon purchase"><Icon icon="fluent:cart-24-regular" width="21" /></div><div className="small-card-copy"><span className="card-kicker">MORE FROM TURTLE FLOW</span><h3>Looking for another license?</h3><p>New license purchases will be available here soon.</p><span className="coming-soon"><span /> COMING SOON</span></div></article>
        </section>

        <footer className="page-footer"><span>© 2026 Turtle Flow · Purchases are not enabled</span><div><a href="https://turtle-flow.com/privacy">Privacy</a><a href="https://turtle-flow.com/terms">Terms</a><a href="mailto:support@turtle-flow.com">Support</a></div></footer>
      </div>
    </main></ProtectionGate>
  )
}
