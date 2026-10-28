import { Icon } from '../lib/fluent-icon'

export function AuthFrame({ eyebrow, title, subtitle, children }: { eyebrow: string; title: string; subtitle: string; children: React.ReactNode }) {
  return <main className="auth-main"><a className="auth-brand" href="/" aria-label="Turtle Flow home"><img className="brand-logo" src="/assets/turtle-flow-logo.webp" width="51" height="24" alt="" /><span className="brand-name">Turtle <span>Flow</span></span></a><section className="auth-card"><div className="auth-eyebrow"><span className="eyebrow-dot" />{eyebrow}</div><h1>{title}</h1><p className="auth-subtitle">{subtitle}</p>{children}<div className="auth-safe"><Icon icon="fluent:shield-lock-24-regular" width="15" /> Protected by encrypted sessions</div></section><div className="auth-legal">© 2026 Turtle Flow <span>·</span> <a href="https://turtle-flow.com/privacy">Privacy</a> <span>·</span> <a href="https://turtle-flow.com/terms">Terms</a></div></main>
}
