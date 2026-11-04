import { Icon } from '../lib/fluent-icon'
import { createFileRoute, redirect } from '@tanstack/react-router'
import { ProtectionGate } from '../components/ProtectionGate'
import { getAccountProtectionStatus } from '../server/auth.functions'
import { getMyBillingAccess } from '../server/billing.functions'

export const Route = createFileRoute('/billing')({
  beforeLoad: async () => {
    const security = await getAccountProtectionStatus()
    if (!security.session) throw redirect({ to: '/login' })
    const access = security.isProtected ? await getMyBillingAccess() : { status: 'disabled' as const }
    return { isProtected: security.isProtected, access }
  },
  component: BillingPage,
})

function BillingPage() {
  const { isProtected, access } = Route.useRouteContext()
  return <ProtectionGate protectedAccount={isProtected}><main className="main-shell"><header className="topbar"><div className="breadcrumbs"><a href="/">Account</a><Icon icon="fluent:chevron-right-16-regular" width="14" /><strong>Billing</strong></div><span className="secure-label"><Icon icon="fluent:receipt-24-regular" width="14" /> Purchase history</span></header><div className="content-wrap settings-content"><div className="eyebrow"><span className="eyebrow-dot" /> ACCOUNT BILLING</div><h1>Billing<span>.</span></h1><p className="intro settings-intro">Manage payment methods and view receipts for Turtle Flow.</p><p role="status">{({ disabled: "Billing is not connected yet.", allowed: "Your account has verified Team access.", denied: "No active Team access was found for your account.", unavailable: "Account access could not be verified. Try again later." })[access.status]}</p><section className="empty-state"><span className="empty-icon"><Icon icon="fluent:receipt-24-regular" width="27" /></span><h2>Billing will open with the license store</h2><p>There is no payment provider or live checkout yet. Product plans, pricing, and checkout remain private until they are approved.</p><span className="coming-soon"><span /> NOT AVAILABLE YET</span></section></div></main></ProtectionGate>
}
