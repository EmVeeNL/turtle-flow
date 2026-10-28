import { Icon } from '../lib/fluent-icon'
import { Link, useRouterState } from '@tanstack/react-router'
import { authClient } from '../lib/auth-client'

const navItems = [
  { label: 'Overview', icon: 'fluent:home-24-regular', to: '/' },
  { label: 'Licenses', icon: 'fluent:key-24-regular', to: '/licenses' },
  { label: 'Billing', icon: 'fluent:receipt-24-regular', to: '/billing' },
  { label: 'Security', icon: 'fluent:shield-lock-24-regular', to: '/security' },
]

export default function Header() {
  const pathname = useRouterState({ select: (state) => state.location.pathname })
  if (['/login', '/signup', '/two-factor', '/forgot-password', '/reset-password'].includes(pathname)) return null

  return (
    <aside className="sidebar">
      <a className="brand" href="/" aria-label="Turtle Flow account home">
        <img className="brand-logo" src="/assets/turtle-flow-logo.webp" width="51" height="24" alt="" />
        <span className="brand-name">Turtle <span>Flow</span></span>
      </a>
      <div className="workspace-label">ACCOUNT</div>
      <nav className="side-nav" aria-label="Account navigation">
        {navItems.map((item) => (
          <Link key={item.label} to={item.to} className={`side-link${pathname === item.to ? ' active' : ''}`} aria-current={pathname === item.to ? 'page' : undefined}>
            <Icon icon={item.icon} width="19" height="19" />{item.label}
          </Link>
        ))}
      </nav>
      <div className="sidebar-spacer" />
      <div className="help-card">
        <div className="help-icon"><Icon icon="fluent:chat-help-24-regular" width="20" /></div>
        <div><strong>Need a hand?</strong><p>We’re here to help you get moving.</p><a href="mailto:support@turtle-flow.com">Contact support <Icon icon="fluent:arrow-up-right-16-regular" width="13" /></a></div>
      </div>
      <div className="profile-row">
        <div className="avatar">TF</div>
        <div className="profile-copy"><strong>Turtle Flow user</strong><span>Personal account</span></div>
        <button className="icon-button profile-menu" aria-label="Sign out" title="Sign out" onClick={async () => { await authClient.signOut(); window.location.assign('/login') }}><Icon icon="fluent:arrow-exit-20-regular" width="19" /></button>
      </div>
    </aside>
  )
}
