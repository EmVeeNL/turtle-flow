# Generates the dashboard mockup HTML files. Usage: python3 generate.py <output-dir>
import sys, pathlib, random, json
out = pathlib.Path(sys.argv[1])
ICONS = json.loads((pathlib.Path(__file__).parent / "fluent-icons.json").read_text())["icons"]

def ic(name, size=18):
    """Inline Fluent UI System Icon (via Iconify JSON), as the dashboard will render it."""
    return (f'<svg class="ic" width="{size}" height="{size}" viewBox="0 0 20 20" aria-hidden="true">'
            f'{ICONS[name]["body"]}</svg>')

NAV = [("Overview", None, "board-20-regular"), ("Projects", None, "folder-20-regular"), ("Jobs", None, "task-list-ltr-20-regular"),
       ("Approvals", "3", "approvals-app-20-regular"), ("Runs", None, "bot-20-regular"), ("Analytics", None, "data-area-20-regular"),
       ("Providers", None, "plug-connected-20-regular"), ("Resources", None, "document-multiple-20-regular"), ("Settings", None, "settings-20-regular")]

def shell(title, active, crumbs, body):
    nav = "".join(
        f'<a class="{"active" if n == active else ""}">{ic(i)}{n}'
        + (f'<span class="badge">{b}</span>' if b else "") + "</a>" for n, b, i in NAV)
    return f"""<!doctype html><html lang="en"><head><meta charset="utf-8"><title>{title}</title>
<link rel="stylesheet" href="shared.css"></head><body>
<aside class="sidebar"><div class="brand"><div class="logo" role="img" aria-label="Turtle Flow logo"></div>Turtle Flow<span class="kbd">⌘K</span></div>
<nav class="nav">{nav}</nav>
<div class="side-foot"><div><span class="dot"></span>daemon connected · v0.3.0</div><div>License: <b style="color:var(--text)">Free</b> · local</div></div></aside>
<div class="main"><header class="topbar"><div class="crumbs">{crumbs}</div>
<div class="search">{ic("search-20-regular",16)}&nbsp; Search jobs, runs, resources…</div>
<div class="live"><span class="dot"></span>Live</div></header>
<main class="content">{body}</main></div></body></html>"""

def stepper(states):
    return '<span class="mstep">' + "".join(f'<i class="{s}"></i>' for s in states) + "</span>"

# ---------------------------------------------------------------- 1 overview
def overview():
    jobs = [
        ("API-42", "Add per-client rate limiting", "payments-api", "dddddddddcw", "acceptance approval", "pill p-warn", "codex · claude-code", "612k", "2h 14m"),
        ("WEB-17", "Dark mode for settings pages", "web-console", "dddw", "scope approval", "pill p-warn", "claude-code", "41k", "12m"),
        ("API-44", "Idempotency keys for refunds", "payments-api", "dddddc", "implementation 3/5", "pill p-ai", "codex", "288k", "48m"),
        ("CLI-8", "Parallel upload with retries", "sync-cli", "ddddddw", "awaiting budget", "pill p-warn", "codex", "402k", "1h 05m"),
        ("WEB-12", "Fix flaky checkout e2e tests", "web-console", "ddddddx", "blocked · fix limit", "pill p-bad", "claude-code", "355k", "1h 32m"),
        ("DOC-3", "Document webhook retries", "docs-site", "ddc", "specification", "pill p-ai", "claude-code", "9k", "3m"),
        ("API-45", "Upgrade sqlx to 0.8", "payments-api", "d", "queued", "pill p-mute", "—", "—", "—"),
    ]
    rows = "".join(f"""<tr><td class="mono acc">{k}</td><td>{t}</td><td class="muted">{p}</td><td>{stepper(list(s))}</td>
<td><span class="{pc}">{st}</span></td><td class="muted">{pr}</td><td class="r mono">{tok}</td><td class="r muted">{el}</td></tr>""" for k, t, p, s, st, pc, pr, tok, el in jobs)
    approvals = [
        ("Acceptance", "API-42 · Add per-client rate limiting", "diff sha256:5d0c…e81a · 7 files +295 −1", "2 flags: dependency added, protected path", "42 min"),
        ("Scope", "WEB-17 · Dark mode for settings pages", "spec rev 1 · sha256:91af…3c07 · est. 120k–310k", "No flags", "12 min"),
        ("Budget increase", "CLI-8 · Parallel upload with retries", "+300k tokens · job at 1.1M of 1.2M", "Remaining work est. 140k–260k", "6 min"),
    ]
    acards = "".join(f"""<div style="padding:9px 16px;border-bottom:1px solid var(--border)">
<div style="display:flex;justify-content:space-between;align-items:center"><span class="pill p-warn">◆ {g}</span><span class="faint" style="font-size:12px">waiting {a}</span></div>
<div style="margin-top:5px;font-weight:500">{t}</div><div style="display:flex;justify-content:space-between;align-items:center;margin-top:3px"><span class="mono muted">{h}</span><span class="btn" style="height:26px">Review →</span></div><div class="muted" style="font-size:12px;margin-top:2px">{f}</div></div>""" for g, t, h, f, a in approvals)
    feed = [
        ("09:15:41", "ai", "API-44 · implementer · codex", "Added idempotency key table migration"),
        ("09:15:12", "ok", "API-44 · gate test", "cargo test — 412 passed"),
        ("09:14:58", "ai", "DOC-3 · spec_writer · claude-code", "Drafting acceptance criteria (4)"),
        ("09:13:30", "warn", "CLI-8 · ledger", "Reservation denied on job budget"),
        ("09:12:02", "bad", "WEB-12 · gate test", "checkout.spec.ts — 2 failed (fix 3/3)"),
        ("09:09:15", "ai", "API-42 · reviewer · claude-code", "0 blockers · 1 major (fixed) · 3 minor"),
    ]
    feedh = "".join(f"""<div style="display:flex;gap:10px;padding:8px 16px;border-bottom:1px solid var(--border);font-size:12.5px">
<span class="mono faint" style="font-size:11.5px;width:58px">{t}</span><span class="dot" style="background:var(--{'ai' if c=='ai' else 'success' if c=='ok' else 'warning' if c=='warn' else 'danger'});margin-top:5px"></span>
<div><div class="muted">{w}</div><div>{m}</div></div></div>""" for t, c, w, m in feed)
    budgets = [("payments-api", 6.4, 0.4, 25, "hard"), ("web-console", 3.9, 0.1, 10, "hard"), ("sync-cli", 4.1, 0.3, 6, "hard"), ("docs-site", 0.4, 0.0, 2, "soft")]
    bud = "".join(f"""<div style="margin-bottom:12px"><div style="display:flex;justify-content:space-between;font-size:12.5px"><span>{n}</span>
<span class="muted"><span class="mono">{s}M</span> + <span class="mono">{r}M</span> reserved / <span class="mono">{l}M</span> · {h}</span></div>
<div class="bar" style="margin-top:5px"><div class="s" style="width:{s/l*100:.1f}%;{'background:var(--warning)' if (s+r)/l>0.6 else ''}"></div><div class="rsv" style="left:{s/l*100:.1f}%;width:{r/l*100:.1f}%"></div></div></div>""" for n, s, r, l, h in budgets)
    body = f"""
<div style="display:flex;align-items:center;justify-content:space-between;margin-bottom:16px"><div><h1>Good morning</h1><div class="muted" style="margin-top:2px">3 approvals need you · 3 agents running · all within budget</div></div>
<div style="display:flex;gap:8px"><span class="btn">{ic("pause-20-regular",16)} Pause all</span><span class="btn primary">{ic("add-20-regular",16)} New job</span></div></div>
<div class="kpis" style="margin-bottom:16px">
<div class="card kpi"><div class="l">Active jobs</div><div class="v">7</div><div class="s">3 running · 2 awaiting approval · 1 blocked · 1 queued</div></div>
<div class="card kpi"><div class="l">Pending approvals</div><div class="v warn">3</div><div class="s">oldest waiting 42 min</div></div>
<div class="card kpi"><div class="l">Tokens this month <span class="pill p-mute" style="margin-left:4px">measured</span></div><div class="v">21.3M <span class="muted" style="font-size:15px;font-weight:400">/ 60M soft</span></div>
<div class="bar" style="margin-top:8px"><div class="s" style="width:35.5%"></div><div class="rsv" style="left:35.5%;width:1.3%"></div></div></div>
<div class="card kpi"><div class="l">Verified jobs · 30 days</div><div class="v ok">18</div><div class="s">≈ 690k tokens per verified job (all attempts)</div></div></div>
<div class="row" style="height:782px">
<div class="col" style="flex:1.75">
<div class="row" style="height:388px">
<div class="card" style="flex:1"><div class="card-h"><h2>Needs your attention</h2><span class="muted" style="font-size:12px">3 approvals · 1 blocked</span></div>{acards}</div>
<div class="card" style="width:420px"><div class="card-h"><h2>Blocked</h2><span class="pill p-bad">1</span></div>
<div class="card-b"><div class="mono acc">WEB-12</div><div style="font-weight:500;margin-top:2px">Fix flaky checkout e2e tests</div>
<div class="muted" style="font-size:12.5px;margin-top:8px">Gate <b style="color:var(--text)">test</b> failed after 3/3 fix iterations</div>
<div class="mono" style="margin-top:10px;background:var(--bg);border:1px solid var(--border);border-radius:6px;padding:10px;font-size:11.5px;color:var(--muted);line-height:1.6">✗ checkout.spec.ts › applies coupon<br>✗ checkout.spec.ts › retries payment<br><span class="faint">E_GATE_FIX_LIMIT · from build_test</span></div>
<div style="display:flex;gap:8px;margin-top:12px;flex-wrap:wrap"><span class="btn" style="height:28px">+2 fix iterations</span><span class="btn" style="height:28px">Replan</span><span class="btn" style="height:28px">Open worktree</span></div></div></div></div>
<div class="card" style="flex:1"><div class="card-h"><h2>Active jobs</h2><span class="muted" style="font-size:12px">sorted by priority</span></div>
<table><tr><th>Job</th><th>Title</th><th>Project</th><th>Progress</th><th>Stage</th><th>Providers</th><th class="r">Tokens</th><th class="r">Elapsed</th></tr>{rows}</table></div></div>
<div class="col" style="flex:1">
<div class="card" style="flex:1"><div class="card-h"><h2>Live agent activity</h2><span class="pill p-ai">● 3 running</span></div>{feedh}</div>
<div class="card"><div class="card-h"><h2>Project budgets · October</h2><div class="legend"><span>settled</span><span style="--c:rgba(46,155,255,.4)">reserved</span></div></div><div class="card-b" style="padding-bottom:4px">{bud}</div></div>
<div class="card"><div class="card-h"><h2>Providers</h2></div><table>
<tr><th>Provider</th><th>Status</th><th>Running</th><th>Quota</th></tr>
<tr><td>claude-code <span class="faint mono">2.x</span></td><td><span class="pill p-ok">● healthy</span></td><td class="mono">1 / 2</td><td><span class="pill p-unk">unknown</span></td></tr>
<tr><td>codex <span class="faint mono">0.x</span></td><td><span class="pill p-warn">⏳ rate-limited · until 09:31 (hint)</span></td><td class="mono">2 / 2</td><td><span class="pill p-unk">unknown</span></td></tr>
</table></div></div></div>"""
    return shell("Overview", "Overview", "<b>Overview</b>", body)

# ---------------------------------------------------------------- 2 projects
def projects():
    ps = [
        ("Payments API", "payments-api", "~/dev/payments-api", "Rust · TS", 3, 1, 6.4, 0.4, 25, "96%", "API-39 · 2h ago", "ok"),
        ("Web Console", "web-console", "~/dev/web-console", "TS · React", 2, 1, 3.9, 0.1, 10, "81%", "WEB-15 · yesterday", "warn"),
        ("Sync CLI", "sync-cli", "~/dev/sync-cli", "Go", 1, 1, 4.1, 0.3, 6, "92%", "CLI-6 · 3d ago", "warn"),
        ("Docs Site", "docs-site", "~/dev/docs", "Markdown · Astro", 1, 0, 0.4, 0.0, 2, "100%", "DOC-2 · 5d ago", "ok"),
        ("ML Feature Store", "feature-store", "~/dev/feature-store", "Python", 0, 0, 4.6, 0.0, 8, "88%", "FS-21 · 1w ago", "ok"),
        ("Infra Modules", "infra", "~/dev/infra-modules", "Terraform", 0, 0, 1.9, 0.0, 3, "100%", "INF-4 · 2w ago", "ok"),
        ("Legacy Billing", "legacy-billing", "~/dev/billing-legacy", "Java", 0, 0, 0.0, 0.0, 4, "—", "—", "bad"),
    ]
    rows = ""
    for n, s, path, st, aj, ap, se, re_, lim, gp, last, h in ps:
        pct = se / lim * 100
        health = {"ok": '<span class="pill p-ok">● healthy</span>', "warn": '<span class="pill p-warn">● attention</span>', "bad": '<span class="pill p-bad">● repo missing</span>'}[h]
        rows += f"""<tr style="height:56px"><td><div style="font-weight:500">{n}</div><div class="mono faint" style="font-size:11.5px">{s}</div></td>
<td class="mono muted">{path}</td><td class="muted">{st}</td><td class="r mono">{aj}</td><td class="r">{f'<span class="pill p-warn">{ap}</span>' if ap else '<span class="faint">0</span>'}</td>
<td style="width:260px"><div style="display:flex;justify-content:space-between;font-size:12px"><span class="mono">{se}M <span class="faint">+{re_}M</span></span><span class="muted mono">{lim}M</span></div>
<div class="bar" style="margin-top:4px"><div class="s" style="width:{pct:.1f}%"></div><div class="rsv" style="left:{pct:.1f}%;width:{re_/lim*100:.1f}%"></div></div></td>
<td class="r mono">{gp}</td><td class="muted">{last}</td><td>{health}</td></tr>"""
    body = f"""
<div style="display:flex;align-items:center;justify-content:space-between;margin-bottom:16px"><div><h1>Projects</h1><div class="muted" style="margin-top:2px">7 projects · 6 active · each project is isolated (repo, budgets, resources, knowledge)</div></div>
<div style="display:flex;gap:8px"><span class="btn">{ic("arrow-download-20-regular",16)} Export</span><span class="btn primary">{ic("add-20-regular",16)} Add project</span></div></div>
<div style="display:flex;gap:8px;margin-bottom:12px"><span class="btn" style="width:280px;color:var(--faint)">Filter projects…</span><span class="btn">Status: Active ▾</span><span class="btn">Stack: All ▾</span><span class="btn">Sort: Activity ▾</span>
<span style="margin-left:auto" class="btn">☰ Table</span><span class="btn" style="color:var(--muted)">▦ Cards</span></div>
<div class="card"><table><tr><th>Project</th><th>Repository</th><th>Stack</th><th class="r">Active jobs</th><th class="r">Approvals</th><th>Tokens this month (settled + reserved / limit)</th><th class="r">Gate pass 30d</th><th>Last delivery</th><th>Health</th></tr>{rows}</table></div>
<div class="row" style="margin-top:16px">
<div class="card" style="flex:1"><div class="card-h"><h2>Add a project from the CLI</h2></div><div class="card-b mono" style="line-height:1.9">
<span class="faint">$</span> turtle project add ~/dev/new-service --profile standard<br><span class="faint">✓ detected: Cargo workspace · build/test/lint commands proposed · default branch main (protected)</span></div></div>
<div class="card" style="flex:1"><div class="card-h"><h2>Legacy Billing</h2><span class="pill p-bad">action needed</span></div><div class="card-b" style="font-size:13px">
Repository not found at <span class="mono">~/dev/billing-legacy</span>. Jobs are paused for this project.<div style="display:flex;gap:8px;margin-top:10px"><span class="btn" style="height:28px">Relink path</span><span class="btn" style="height:28px">Archive</span></div></div></div>
<div class="card" style="flex:1"><div class="card-h"><h2>Spend split · October</h2><span class="pill p-mute">API-equivalent</span></div><div class="card-b">
<div style="display:flex;height:14px;border-radius:4px;overflow:hidden"><div style="width:30%;background:#2E9BFF"></div><div style="width:18%;background:#A78BFA"></div><div style="width:19%;background:#39C5CF"></div><div style="width:22%;background:#3FB950"></div><div style="width:11%;background:#6E7681"></div></div>
<div class="legend" style="margin-top:10px;flex-wrap:wrap"><span>payments-api 30%</span><span style="--c:#A78BFA">web-console 18%</span><span style="--c:#39C5CF">sync-cli 19%</span><span style="--c:#3FB950">feature-store 22%</span><span style="--c:#6E7681">other 11%</span></div></div></div></div>
<div class="card" style="margin-top:16px"><div class="card-h"><h2>Recent deliveries</h2><span class="muted" style="font-size:12px">all projects · last 7 days</span></div><table>
<tr><th>Job</th><th>Title</th><th>Project</th><th>Mode</th><th>Result</th><th>Approved by</th><th class="r">Tokens</th><th class="r">When</th></tr>
<tr><td class="mono acc">API-39</td><td>Webhook signature rotation</td><td class="muted">payments-api</td><td><span class="pill p-mute">pr</span></td><td class="mono muted">#412 merged</td><td class="muted">you · dashboard</td><td class="r mono">488k</td><td class="r muted">2h ago</td></tr>
<tr><td class="mono acc">WEB-15</td><td>Accessible focus states in nav</td><td class="muted">web-console</td><td><span class="pill p-mute">local_squash</span></td><td class="mono muted">c71e0a2 on main</td><td class="muted">you · cli</td><td class="r mono">212k</td><td class="r muted">yesterday</td></tr>
<tr><td class="mono acc">CLI-6</td><td>Resume interrupted uploads</td><td class="muted">sync-cli</td><td><span class="pill p-mute">pr</span></td><td class="mono muted">#88 open</td><td class="muted">you · dashboard</td><td class="r mono">903k</td><td class="r muted">3d ago</td></tr>
<tr><td class="mono acc">DOC-2</td><td>Quickstart for webhooks</td><td class="muted">docs-site</td><td><span class="pill p-mute">local_ff</span></td><td><span class="pill p-warn">pending integration</span></td><td class="muted">you · cli</td><td class="r mono">64k</td><td class="r muted">5d ago</td></tr>
</table></div>"""
    return shell("Projects", "Projects", "<b>Projects</b>", body)

# ---------------------------------------------------------------- 3 job detail
def job_detail():
    stages = [("Backlog", "d"), ("Discovery", "d"), ("Specification", "d"), ("Scope approval", "d◆"), ("Planning", "d"), ("Implementation", "d↺1"),
              ("Build & tests", "d"), ("Review", "d↺1"), ("Verification", "d"), ("Acceptance", "w◆"), ("Delivery", "")]
    st = ""
    for i, (n, s) in enumerate(stages):
        col = "var(--success)" if s.startswith("d") else "var(--warning)" if s.startswith("w") else "var(--border)"
        loop = s[2:] if "↺" in s else (s[1:] if "↺" in s[1:] else "")
        dia = "◆" in s
        mark = f'<div style="width:{16 if dia else 14}px;height:{16 if dia else 14}px;{"transform:rotate(45deg);border-radius:3px" if dia else "border-radius:50%"};background:{col};{"box-shadow:0 0 0 4px rgba(210,153,34,.2)" if s.startswith("w") else ""}"></div>'
        st += f"""<div style="flex:1;display:flex;flex-direction:column;align-items:center;gap:8px;position:relative">
{'' if i == 0 else f'<div style="position:absolute;top:7px;right:50%;width:100%;height:2px;background:{col if s else "var(--border)"};z-index:0"></div>'}
<div style="z-index:1">{mark}</div><div style="font-size:12px;{'color:var(--text);font-weight:600' if s.startswith('w') else 'color:var(--muted)'}">{n}</div>
{f'<span class="pill p-mute" style="font-size:10.5px;padding:0 6px">↺ {loop.replace("↺","")}</span>' if "↺" in s else '<span style="height:17px"></span>'}</div>"""
    tasks = [("1", "Add rate limit config to ClientTier", "codex", "a91c2e0", "ok", "build ✓ test ✓", "38k"),
             ("2", "Add token bucket middleware", "codex", "c3f08d1", "ok", "build ✓ test ✓ (fix 1)", "96k"),
             ("3", "Register middleware + exemptions", "codex", "7e21b4a", "ok", "build ✓ test ✓", "31k"),
             ("4", "Prometheus counter api_rate_limited_total", "codex", "0d9a6f3", "ok", "build ✓ test ✓", "27k"),
             ("5", "Integration tests for 429 + Retry-After", "codex", "e5b7712", "ok", "build ✓ test ✓", "44k")]
    trows = "".join(f"""<tr><td class="mono faint">{n}</td><td>{t}</td><td class="muted">{p}</td><td class="mono acc">{c}</td><td class="ok" style="font-size:12.5px">{g}</td><td class="r mono">{tk}</td></tr>""" for n, t, p, c, s, g, tk in tasks)
    tl = [("09:10", "ok", "Verification", "6/6 criteria met · report rev 1"), ("09:09", "ai", "Review (claude-code)", "0 blockers · 1 major fixed · 3 minor"),
          ("08:58", "warn", "Review → Implementation", "1 major finding: Retry-After uses wall clock"), ("08:51", "ok", "Build & tests", "build · test · lint · fmt · contract ✓"),
          ("08:12", "ai", "Task 2 fix 1/3", "test failed: missing Retry-After header"), ("07:41", "ok", "Worktree created", "turtle/API-42-add-rate-limiting @ 3f2a9c1"),
          ("07:38", "ok", "Plan rev 2", "5 tasks · est. 180k–340k"), ("07:22", "ok", "Scope approved", "spec rev 3 · sha256:91af…3c07 · by you (cli)"),
          ("07:05", "ok", "Specification rev 3", "6 acceptance criteria · your edit")]
    tlh = "".join(f"""<div style="display:flex;gap:10px;padding:9px 0;border-bottom:1px solid var(--border)"><span class="mono faint" style="font-size:11.5px;width:40px">{t}</span>
<span class="dot" style="margin-top:6px;background:var(--{'ai' if c=='ai' else 'success' if c=='ok' else 'warning'})"></span><div><div style="font-size:13px;font-weight:500">{h}</div><div class="muted" style="font-size:12.5px">{d}</div></div></div>""" for t, c, h, d in tl)
    roles = [("spec_writer", 46, "claude-code"), ("planner", 38, "claude-code"), ("implementer", 236, "codex"), ("fixer", 71, "codex"), ("reviewer", 154, "claude-code"), ("verifier", 22, "codex")]
    rh = "".join(f"""<div style="display:flex;align-items:center;gap:10px;margin-bottom:8px;font-size:12.5px"><span style="width:84px" class="muted">{r}</span>
<div style="flex:1;height:8px;background:var(--surface-2);border-radius:4px"><div style="width:{v/236*100:.0f}%;height:8px;border-radius:4px;background:{'var(--accent)' if p=='claude-code' else 'var(--ai)'}"></div></div><span class="mono" style="width:44px;text-align:right">{v}k</span></div>""" for r, v, p in roles)
    body = f"""
<div style="display:flex;align-items:flex-start;justify-content:space-between"><div>
<div style="display:flex;align-items:center;gap:10px"><span class="mono acc" style="font-size:14px">API-42</span><h1>Add per-client rate limiting to the public API</h1></div>
<div style="display:flex;gap:8px;margin-top:8px;align-items:center"><span class="pill p-warn">◆ acceptance approval</span><span class="pill p-mute">profile standard-api</span><span class="pill p-mute">complexity M</span>
<span class="pill p-mute">feature · high</span><span class="muted" style="font-size:12.5px">payments-api · created 07:01 · elapsed 2h 14m · ticket PAY-1182</span></div></div>
<div style="display:flex;gap:8px"><span class="btn">{ic("pause-20-regular",16)} Pause</span><span class="btn">{ic("arrow-download-20-regular",16)} Export report</span><span class="btn danger">Cancel</span><span class="btn primary">Review &amp; accept →</span></div></div>
<div class="card" style="margin-top:16px;padding:18px 12px 12px"><div style="display:flex">{st}</div></div>
<div class="row" style="margin-top:16px;height:742px">
<div class="card" style="width:430px;display:flex;flex-direction:column"><div class="card-h"><h2>Timeline</h2><span class="muted" style="font-size:12px">47 events</span></div><div style="padding:0 16px;overflow:hidden">{tlh}</div></div>
<div class="col" style="flex:1">
<div class="card"><div class="card-h"><h2>Tasks · plan rev 2</h2><span class="pill p-ok">5 / 5 succeeded</span></div>
<table><tr><th>#</th><th>Task</th><th>Agent</th><th>Commit</th><th>Gates</th><th class="r">Tokens</th></tr>{trows}</table></div>
<div class="card" style="flex:1"><div class="card-h"><h2>Artifacts</h2><span class="muted" style="font-size:12px">immutable revisions</span></div><table>
<tr><th>Artifact</th><th>Revision</th><th>Hash</th><th>Author</th><th>Status</th></tr>
<tr><td>Specification</td><td class="mono">rev 3</td><td><span class="hash">sha256:91af…3c07</span></td><td class="muted">you (edit of rev 2)</td><td><span class="pill p-ok">scope approved</span></td></tr>
<tr><td>Preflight report</td><td class="mono">rev 2</td><td><span class="hash">sha256:2c71…a90e</span></td><td class="muted">system</td><td><span class="pill p-mute">attached</span></td></tr>
<tr><td>Plan</td><td class="mono">rev 2</td><td><span class="hash">sha256:0b2d…77f1</span></td><td class="muted">planner · claude-code</td><td><span class="pill p-mute">active</span></td></tr>
<tr><td>Review report</td><td class="mono">rev 2</td><td><span class="hash">sha256:e19a…5b42</span></td><td class="muted">reviewer · claude-code</td><td><span class="pill p-ok">0 blockers</span></td></tr>
<tr><td>Verification report</td><td class="mono">rev 1</td><td><span class="hash">sha256:c4e0…19d3</span></td><td class="muted">verifier · codex + gates</td><td><span class="pill p-ok">6/6 met</span></td></tr>
<tr><td>Job diff</td><td class="mono">a81d77e</td><td><span class="hash">sha256:5d0c…e81a</span></td><td class="muted">Turtle Flow</td><td><span class="pill p-warn">awaiting acceptance</span></td></tr>
</table></div></div>
<div class="col" style="width:420px">
<div class="card"><div class="card-h"><h2>Tokens</h2><span class="pill p-mute">measured 567k</span></div><div class="card-b">
<div class="muted" style="font-size:12.5px">Preflight estimate (P10–P90) vs. actual</div>
<div class="est" style="margin-top:8px"><div class="track"></div><div class="range" style="left:22%;width:50%"></div><div class="p50" style="left:38%"></div><div class="act" style="left:41%"></div></div>
<div style="display:flex;justify-content:space-between;font-size:11.5px" class="mono muted"><span>≈310k</span><span>P50 ≈520k</span><span>≈910k</span></div>
<div class="legend" style="margin:8px 0 14px"><span style="--c:transparent;outline:1.5px dashed var(--accent)">estimate range · medium confidence</span><span style="--c:var(--success)">actual</span></div>
{rh}
<div class="legend" style="margin-top:6px"><span>claude-code</span><span style="--c:var(--ai)">codex</span></div>
<div style="margin-top:14px;font-size:12.5px;display:flex;justify-content:space-between"><span class="muted">Job budget (hard)</span><span class="mono">567k / 1.2M</span></div>
<div class="bar" style="margin-top:5px"><div class="s" style="width:47%"></div></div>
<div style="margin-top:8px;font-size:12px" class="muted">API-equivalent cost ≈ $3.40 · retries 13% of total</div></div></div>
<div class="card" style="flex:1"><div class="card-h"><h2>Approvals</h2></div><div class="card-b" style="font-size:13px;display:grid;gap:12px">
<div><div style="display:flex;justify-content:space-between"><span>Scope</span><span class="pill p-ok">approved · valid</span></div><div class="mono muted" style="margin-top:3px">spec rev 3 · 91af…3c07 · envelope 1.14M</div></div>
<div><div style="display:flex;justify-content:space-between"><span>Acceptance</span><span class="pill p-warn">pending · 42 min</span></div><div class="mono muted" style="margin-top:3px">diff 5d0c…e81a · base 3f2a9c1 · head a81d77e</div>
<div class="muted" style="margin-top:6px;font-size:12px">Flags: new dependency <span class="mono">governor 0.6</span> · protected path <span class="mono">Cargo.lock</span></div></div></div></div></div></div>"""
    return shell("Job API-42", "Jobs", "Projects / payments-api / <b>API-42</b>", body)

# ---------------------------------------------------------------- 4 approvals
def approvals():
    q = [("Acceptance", "API-42", "Add per-client rate limiting", "42 min", "2 flags", True),
         ("Scope", "WEB-17", "Dark mode for settings pages", "12 min", "", False),
         ("Budget increase", "CLI-8", "Parallel upload with retries", "6 min", "+300k", False)]
    qh = "".join(f"""<div style="padding:12px 14px;border-bottom:1px solid var(--border);{'background:var(--surface-2);box-shadow:inset 3px 0 0 var(--accent)' if a else ''}">
<div style="display:flex;justify-content:space-between"><span class="pill p-warn">◆ {g}</span><span class="faint" style="font-size:12px">{w}</span></div>
<div style="margin-top:7px"><span class="mono acc">{k}</span> {t}</div>{f'<div class="warn" style="font-size:12px;margin-top:4px">⚑ {f}</div>' if f else ''}</div>""" for g, k, t, w, f, a in q)
    crit = [("AC-1", "Requests above the per-key limit receive HTTP 429", "test", "tests/rate_limit.rs::over_limit_returns_429"),
            ("AC-2", "429 responses include Retry-After in seconds", "test", "tests/rate_limit.rs::retry_after_header"),
            ("AC-3", "Limits configurable per ClientTier", "test", "tests/rate_limit.rs::tier_limits"),
            ("AC-4", "/health and /metrics are exempt", "test", "tests/rate_limit.rs::exemptions"),
            ("AC-5", "Counter api_rate_limited_total{tier} exported", "command", "metrics scrape check"),
            ("AC-6", "Existing test suite remains green", "command", "cargo test --workspace (412 passed)")]
    ch = "".join(f"""<tr><td class="mono acc">{i}</td><td>{s}</td><td><span class="pill p-mute">{m}</span></td><td class="mono muted" style="font-size:12px">{e}</td><td><span class="pill p-ok">✓ met</span></td></tr>""" for i, s, m, e in crit)
    body = f"""
<div style="display:flex;align-items:center;justify-content:space-between;margin-bottom:16px"><div><h1>Approvals</h1><div class="muted" style="margin-top:2px">Approvals bind to exact artifact hashes. Any material change invalidates them.</div></div>
<div style="display:flex;gap:8px"><span class="btn">Pending (3)</span><span class="btn" style="color:var(--muted)">History</span></div></div>
<div class="row" style="height:900px">
<div class="card" style="width:360px">{qh}</div>
<div class="col" style="flex:1">
<div class="card"><div class="card-h"><div><span class="pill p-warn">◆ Acceptance approval</span> <span class="mono acc" style="margin-left:8px">API-42</span> <span style="font-weight:500;margin-left:6px">Add per-client rate limiting to the public API</span></div><span class="muted" style="font-size:12px">requested 08:33 · expires in 6 d 23 h</span></div>
<div class="card-b"><div class="row">
<div style="flex:1.3"><h2 style="margin-bottom:10px">What you are approving</h2>
<table style="font-size:12.5px"><tr><td class="muted" style="height:30px">Target</td><td><span class="mono">main</span> <span class="pill p-mute">protected</span> via pull request</td></tr>
<tr><td class="muted" style="height:30px">Commits</td><td class="mono">base 3f2a9c1 → head a81d77e (5 commits)</td></tr>
<tr><td class="muted" style="height:30px">Diff</td><td><span class="hash">sha256:5d0c41e7…b39e81a</span> · 7 files · <span class="ok">+295</span> <span class="bad">−1</span></td></tr>
<tr><td class="muted" style="height:30px">Specification</td><td><span class="hash">sha256:91af…3c07</span> rev 3</td></tr>
<tr><td class="muted" style="height:30px">Verification</td><td><span class="hash">sha256:c4e0…19d3</span> rev 1 · 6/6 criteria met</td></tr>
<tr><td class="muted" style="height:30px">Review</td><td><span class="hash">sha256:e19a…5b42</span> rev 2 · 0 blockers · 3 minor open</td></tr>
<tr><td class="muted" style="height:30px">Gates @ a81d77e</td><td class="ok">build ✓ test ✓ lint ✓ fmt ✓ contract_tests ✓ secret_scan ✓</td></tr>
<tr><td class="muted" style="height:30px">Binding hash</td><td><span class="hash">subject sha256:7a4f…0c2d</span></td></tr></table></div>
<div style="flex:1"><h2 style="margin-bottom:10px">Flags requiring acknowledgement</h2>
<div style="border:1px solid rgba(210,153,34,.4);background:rgba(210,153,34,.06);border-radius:8px;padding:12px;font-size:13px;display:grid;gap:10px">
<label style="display:flex;gap:10px"><span style="width:16px;height:16px;border:1.5px solid var(--warning);border-radius:4px;flex:none;margin-top:2px;background:var(--warning)"></span><div><b>New dependency</b> <span class="mono">governor = "0.6"</span> (crates.io, 2.1M downloads/month)</div></label>
<label style="display:flex;gap:10px"><span style="width:16px;height:16px;border:1.5px solid var(--warning);border-radius:4px;flex:none;margin-top:2px"></span><div><b>Protected path</b> <span class="mono">Cargo.lock</span> changed (+38 lines, dependency resolution only)</div></label></div>
<h2 style="margin:16px 0 8px">Invalidated if</h2><ul class="muted" style="font-size:12.5px;padding-left:18px;line-height:1.8"><li>any new commit or working-tree change in the job worktree</li><li><span class="mono">main</span> moves and the branch is refreshed</li><li>spec, plan or verification report get a new revision</li><li>policy version changes (protected paths)</li></ul></div></div></div></div>
<div class="card" style="flex:1"><div class="card-h"><h2>Acceptance criteria · verification evidence</h2><span class="pill p-ok">6 / 6 met</span></div>
<table><tr><th>ID</th><th>Criterion</th><th>Method</th><th>Evidence</th><th>Status</th></tr>{ch}</table>
<div style="padding:12px 16px;border-top:1px solid var(--border);display:flex;gap:24px;font-size:12.5px" class="muted"><span>Tokens: <b class="mono" style="color:var(--text)">567k</b> measured of 1.14M approved envelope</span><span>Providers: codex (implementer), claude-code (reviewer) — cross-provider ✓</span><span>Fix iterations: 2 · Back-transitions: 1</span></div></div>
<div class="card" style="padding:12px 16px;display:flex;align-items:center;gap:12px"><div style="flex:1;height:36px;border:1px solid var(--border);border-radius:7px;background:var(--bg);color:var(--faint);display:flex;align-items:center;padding:0 12px;font-size:13px">Comment (required for request changes / reject)…</div>
<span class="btn" style="height:36px">Open full diff</span><span class="btn danger" style="height:36px">Reject <span class="k">R</span></span><span class="btn" style="height:36px">Request changes <span class="k">C</span></span><span class="btn primary" style="height:36px;opacity:.55">Approve 7a4f…0c2d <span class="k">A</span></span></div>
<div class="faint" style="font-size:12px;margin-top:-8px">Approve is enabled after all flags are acknowledged (1 of 2). The request will carry subject hash 7a4f…0c2d; if the subject changed meanwhile, nothing is approved.</div>
</div></div>"""
    return shell("Approvals", "Approvals", "<b>Approvals</b> / API-42 acceptance", body)

# ---------------------------------------------------------------- 5 code review
def code_review():
    files = [("src/http/middleware/rate_limit.rs", "A", 148, 0, 2, False), ("src/http/middleware/mod.rs", "M", 1, 0, 0, False),
             ("src/http/router.rs", "M", 6, 1, 1, False), ("src/metrics.rs", "M", 9, 0, 0, False), ("tests/rate_limit.rs", "A", 92, 0, 1, False),
             ("Cargo.toml", "M", 1, 0, 0, True), ("Cargo.lock", "M", 38, 0, 0, True)]
    fh = "".join(f"""<div style="display:flex;align-items:center;gap:8px;padding:7px 12px;font-size:12.5px;{'background:var(--surface-2)' if i==0 else ''}">
<span class="mono {'ok' if s=='A' else 'warn'}" style="width:12px">{s}</span><span class="mono" style="flex:1;overflow:hidden;text-overflow:ellipsis">{p}</span>
{'<span class="pill p-warn" style="padding:0 5px;font-size:10px">prot</span>' if prot else ''}{f'<span class="pill p-ai" style="padding:0 6px">{n}</span>' if n else ''}<span class="mono faint" style="font-size:11px">+{a}</span></div>""" for i, (p, s, a, d, n, prot) in enumerate(files))
    code = [
        (1, "use std::time::{Duration, Instant};", ""), (2, "use governor::{Quota, RateLimiter, clock::DefaultClock};", ""), (3, "", ""),
        (4, "/// Per-API-key token bucket. Limits come from ClientTier (AC-3).", ""), (5, "pub struct RateLimitLayer {", ""),
        (6, "    limiters: DashMap&lt;ApiKeyId, Arc&lt;KeyLimiter&gt;&gt;,", ""), (7, "    tiers: TierLimits,", ""), (8, "}", ""), (9, "", ""),
        (10, "impl RateLimitLayer {", ""), (11, "    pub fn check(&amp;self, key: &amp;ApiKeyId, tier: Tier) -&gt; Decision {", ""),
        (12, "        let limiter = self.limiters.entry(key.clone())", ""), (13, "            .or_insert_with(|| Arc::new(self.tiers.limiter_for(tier)));", "F1"),
        (14, "        match limiter.check() {", ""), (15, "            Ok(()) =&gt; Decision::Allow,", ""),
        (16, "            Err(not_until) =&gt; {", ""), (17, "                let wait = not_until.wait_time_from(DefaultClock::default().now());", ""),
        (18, "                Decision::Reject { retry_after: wait.as_secs().max(1) }", "F2"), (19, "            }", ""), (20, "        }", ""), (21, "    }", ""),
    ]
    ch = ""
    for n, line, f in code:
        ch += f"""<div style="display:flex;background:rgba(63,185,80,.07)"><span class="mono faint" style="width:44px;text-align:right;padding-right:10px;font-size:11.5px;line-height:22px;border-right:3px solid {'var(--warning)' if f=='F1' else 'var(--ai)' if f=='F2' else 'rgba(63,185,80,.5)'}">{n}</span>
<span class="mono ok" style="width:18px;text-align:center;line-height:22px">+</span><span class="mono" style="line-height:22px;white-space:pre;color:#D7DBE3">{line}</span></div>"""
        if f == "F1":
            ch += """<div style="margin:6px 12px 10px 62px;border:1px solid rgba(210,153,34,.45);background:rgba(210,153,34,.06);border-radius:8px;padding:10px 12px;font-size:13px">
<div style="display:flex;gap:8px;align-items:center"><span class="pill p-warn">minor</span><span class="pill p-mute">resource-growth</span><span class="faint" style="font-size:12px">reviewer · claude-code · finding R-3</span><span style="margin-left:auto" class="pill p-mute">open</span></div>
<div style="margin-top:6px">Limiter map grows unbounded with distinct API keys; consider TTL eviction for idle keys.</div>
<div class="mono muted" style="margin-top:6px;font-size:12px">Suggested: replace DashMap with moka::sync::Cache (time_to_idle = 10 min)</div>
<div style="display:flex;gap:8px;margin-top:8px"><span class="btn" style="height:26px;font-size:12px">Request fix</span><span class="btn" style="height:26px;font-size:12px">Dismiss…</span><span class="btn" style="height:26px;font-size:12px">Acknowledge</span></div></div>"""
        if f == "F2":
            ch += """<div style="margin:6px 12px 10px 62px;border:1px solid rgba(63,185,80,.4);background:rgba(63,185,80,.05);border-radius:8px;padding:10px 12px;font-size:13px">
<div style="display:flex;gap:8px;align-items:center"><span class="pill p-bad">major</span><span class="pill p-mute">correctness</span><span class="faint" style="font-size:12px">finding R-1 · review rev 1</span><span style="margin-left:auto" class="pill p-ok">✓ fixed in a81d77e</span></div>
<div style="margin-top:6px" class="muted">Retry-After was computed from SystemTime; now uses the limiter clock and rounds up to ≥ 1 s (AC-2).</div></div>"""
    findings = [("major", "p-bad", "R-1", "Retry-After used wall clock", "fixed"), ("minor", "p-warn", "R-3", "Unbounded limiter map", "open"),
                ("minor", "p-warn", "R-4", "Exemption list hardcoded in router", "open"), ("minor", "p-warn", "R-5", "Missing doc on TierLimits::default", "open"),
                ("nit", "p-mute", "R-6", "Test name casing", "dismissed"), ("info", "p-info", "S-1", "New dependency governor 0.6", "ack required")]
    fl = "".join(f"""<div style="padding:10px 14px;border-bottom:1px solid var(--border)"><div style="display:flex;gap:8px;align-items:center"><span class="pill {c}">{s}</span><span class="mono faint" style="font-size:11.5px">{i}</span><span style="margin-left:auto;font-size:11.5px" class="{'ok' if st=='fixed' else 'warn' if 'ack' in st else 'muted'}">{st}</span></div><div style="margin-top:5px;font-size:13px">{t}</div></div>""" for s, c, i, t, st in findings)
    body = f"""
<div style="display:flex;align-items:center;justify-content:space-between;margin-bottom:12px"><div style="display:flex;align-items:center;gap:10px"><span class="mono acc">API-42</span><h1>Code review</h1>
<span class="pill p-mute">base 3f2a9c1 … head a81d77e</span></div>
<div style="display:flex;gap:8px"><span class="btn">Unified</span><span class="btn" style="color:var(--muted)">Split</span><span class="btn">Request fix (0 selected)</span><span class="btn primary">Proceed to acceptance →</span></div></div>
<div class="card" style="display:flex;gap:28px;padding:10px 16px;margin-bottom:12px;font-size:13px;align-items:center">
<span><b>7</b> files <span class="ok mono">+295</span> <span class="bad mono">−1</span></span><span>Findings: <span class="pill p-bad">0 blocker</span> <span class="pill p-bad">1 major (fixed)</span> <span class="pill p-warn">3 minor</span> <span class="pill p-mute">1 nit</span></span>
<span class="ok">Gates ✓ build · test · lint · fmt · contract</span><span class="muted">Reviewer: claude-code (cross-provider ✓) · 154k tokens</span><span class="muted" style="margin-left:auto">2 of 7 files viewed</span></div>
<div class="row" style="height:860px">
<div class="card" style="width:330px"><div class="card-h"><h2>Files</h2></div>{fh}</div>
<div class="card" style="flex:1;overflow:hidden"><div class="card-h"><span class="mono">src/http/middleware/rate_limit.rs</span><span class="muted" style="font-size:12px">new file · +148 · ☐ viewed</span></div>
<div style="padding:6px 0">{ch}</div></div>
<div class="card" style="width:360px"><div class="card-h"><h2>Findings</h2><span class="btn" style="height:26px;font-size:12px">All severities ▾</span></div>{fl}</div></div>"""
    return shell("Code review API-42", "Jobs", "Projects / payments-api / API-42 / <b>Review</b>", body)

# ---------------------------------------------------------------- 6 analytics
def analytics():
    random.seed(7)
    days = 24
    series = {"claude-code": [], "codex": []}
    for d in range(days):
        base = 0.6 + 0.35 * (d % 7 not in (5, 6))
        series["claude-code"].append(max(0.1, base * (0.55 + random.random() * 0.35)))
        series["codex"].append(max(0.1, base * (0.45 + random.random() * 0.35)))
    W, H, padl, padb = 1040, 260, 40, 24
    mx = 1.6
    def pts(vals, offset):
        return [(padl + i * (W - padl) / (days - 1), H - padb - (v + o) / mx * (H - padb - 10)) for i, (v, o) in enumerate(zip(vals, offset))]
    zero = [0] * days
    c1 = pts(series["claude-code"], zero)
    c2 = pts(series["codex"], series["claude-code"])
    def area(top, bottom):
        return "M" + " L".join(f"{x:.1f},{y:.1f}" for x, y in top) + " L" + " L".join(f"{x:.1f},{y:.1f}" for x, y in reversed(bottom)) + " Z"
    base_line = [(x, H - padb) for x, _ in c1]
    grid = "".join(f'<line x1="{padl}" x2="{W}" y1="{H-padb-g/mx*(H-padb-10):.1f}" y2="{H-padb-g/mx*(H-padb-10):.1f}" stroke="#252A36"/><text x="{padl-8}" y="{H-padb-g/mx*(H-padb-10)+4:.1f}" fill="#6E7681" font-size="11" text-anchor="end">{g:.1f}M</text>' for g in (0.4, 0.8, 1.2, 1.6))
    xl = "".join(f'<text x="{padl + i*(W-padl)/(days-1):.1f}" y="{H-6}" fill="#6E7681" font-size="11" text-anchor="middle">Oct {i+1}</text>' for i in range(0, days, 4))
    chart = f"""<svg width="{W}" height="{H}" viewBox="0 0 {W} {H}">{grid}
<path d="{area(c2, c1)}" fill="rgba(167,139,250,.35)" stroke="#A78BFA" stroke-width="1.5"/>
<path d="{area(c1, base_line)}" fill="rgba(46,155,255,.35)" stroke="#2E9BFF" stroke-width="1.5"/>{xl}</svg>"""
    # scatter estimate vs actual
    SW, SH = 520, 280
    sc = '<line x1="40" y1="250" x2="510" y2="250" stroke="#252A36"/><line x1="40" y1="10" x2="40" y2="250" stroke="#252A36"/>'
    sc += '<path d="M40,250 L510,10" stroke="#6E7681" stroke-dasharray="4 4"/>'
    sc += '<path d="M40,250 L510,60 L510,0 L40,250 Z" fill="none"/>'
    sc += '<polygon points="40,250 510,-60 510,80" fill="rgba(46,155,255,.08)"/>'
    for i in range(46):
        e = random.uniform(0.05, 1.0)
        a = e * random.uniform(0.6, 1.5)
        inside = 0.75 <= a / e <= 1.35
        x = 40 + e * 470; y = 250 - min(a, 1.05) * 228
        sc += f'<circle cx="{x:.1f}" cy="{y:.1f}" r="4.5" fill="{"#3FB950" if inside else "#D29922"}" fill-opacity=".85"/>'
    sc += '<text x="275" y="274" fill="#6E7681" font-size="11" text-anchor="middle">estimated P50 (tokens)</text><text x="14" y="130" fill="#6E7681" font-size="11" transform="rotate(-90 14 130)" text-anchor="middle">actual</text>'
    scatter = f'<svg width="{SW}" height="{SH}" viewBox="0 0 {SW} {SH}" style="overflow:hidden">{sc}</svg>'
    roles = [("implementer", 41, "var(--ai)"), ("reviewer", 19, "var(--accent)"), ("fixer", 14, "var(--warning)"), ("spec_writer", 9, "var(--info)"), ("planner", 8, "var(--success)"), ("verifier", 6, "#6E7681"), ("discovery", 3, "#4A5160")]
    rb = "".join(f"""<div style="display:flex;align-items:center;gap:10px;margin-bottom:9px;font-size:12.5px"><span class="muted" style="width:86px">{r}</span><div style="flex:1;height:10px;background:var(--surface-2);border-radius:4px"><div style="width:{v/41*100:.0f}%;height:10px;border-radius:4px;background:{c}"></div></div><span class="mono" style="width:36px;text-align:right">{v}%</span></div>""" for r, v, c in roles)
    eff = [("codex", "&lt;standard-model&gt;", "implementer", "M", 88, "71%", "1.4", "58k", "$0.31"),
           ("claude-code", "&lt;standard-model&gt;", "implementer", "M", 41, "74%", "1.3", "71k", "$0.44"),
           ("claude-code", "&lt;standard-model&gt;", "reviewer", "M", 52, "—", "—", "96k", "$0.52"),
           ("codex", "&lt;small-model&gt;", "fixer", "S", 63, "82%", "1.1", "14k", "$0.04"),
           ("claude-code", "&lt;fast-model&gt;", "fixer", "S", 37, "79%", "1.2", "19k", "$0.07")]
    et = "".join(f"""<tr><td>{p}</td><td class="mono muted">{m}</td><td>{r}</td><td class="mono">{c}</td><td class="r mono">{n}</td><td class="r mono">{fp}</td><td class="r mono">{fi}</td><td class="r mono">{t}</td><td class="r mono">{co}</td></tr>""" for p, m, r, c, n, fp, fi, t, co in eff)
    body = f"""
<div style="display:flex;align-items:center;justify-content:space-between;margin-bottom:12px"><div><h1>Token usage &amp; cost</h1><div class="muted" style="margin-top:2px">AI provider costs are billed by your providers. Turtle Flow never marks up or resells usage.</div></div>
<div style="display:flex;gap:8px"><span class="btn">Oct 1 – Oct 24 ▾</span><span class="btn">All projects ▾</span><span class="btn">All providers ▾</span><span class="btn">Group: provider ▾</span><span class="btn">{ic("arrow-download-20-regular",16)} CSV</span></div></div>
<div class="kpis" style="margin-bottom:16px">
<div class="card kpi"><div class="l">Total tokens <span class="pill p-mute">97% measured · 3% estimated</span></div><div class="v">21.3M</div><div class="s">input 17.9M (cache read 9.4M) · output 3.4M</div></div>
<div class="card kpi"><div class="l">Cost <span class="pill p-mute">API-equivalent</span></div><div class="v">$118.40</div><div class="s">subscription billing · marginal spend not computed</div></div>
<div class="card kpi"><div class="l">Cost per verified job</div><div class="v">690k <span class="muted" style="font-size:15px;font-weight:400">tokens · ≈ $3.84</span></div><div class="s">fully loaded incl. failed attempts · 18 verified jobs</div></div>
<div class="card kpi"><div class="l">Estimate coverage (P10–P90)</div><div class="v ok">78%</div><div class="s">target 80% · last 50 jobs · calibration ×1.1</div></div></div>
<div class="row" style="height:388px">
<div class="card" style="flex:1"><div class="card-h"><h2>Daily usage by provider</h2><div class="legend"><span>claude-code</span><span style="--c:var(--ai)">codex</span></div></div><div style="padding:12px 16px">{chart}</div></div>
<div class="card" style="width:500px"><div class="card-h"><h2>Share by role</h2><span class="muted" style="font-size:12px">of total tokens</span></div><div class="card-b">{rb}
<div class="muted" style="font-size:12px;margin-top:12px">Orchestration overhead (spec, plan, review, verify, discovery): <b class="mono" style="color:var(--text)">31%</b> · target ≤ 35%</div></div></div></div>
<div class="row" style="margin-top:16px;height:440px">
<div class="card" style="width:560px"><div class="card-h"><h2>Estimate vs. actual · per job</h2><div class="legend"><span style="--c:var(--success)">within P10–P90</span><span style="--c:var(--warning)">outside</span></div></div><div style="padding:8px 16px">{scatter}</div>
<div class="muted" style="font-size:12px;padding:0 16px">Shaded band: typical P10–P90 range. Wider bands for low-confidence buckets.</div></div>
<div class="card" style="flex:1"><div class="card-h"><h2>Provider / model efficiency</h2><span class="muted" style="font-size:12px">buckets with n ≥ 20 used for routing (from 0.4)</span></div>
<table><tr><th>Provider</th><th>Model</th><th>Role</th><th>Class</th><th class="r">Runs</th><th class="r">First-pass gates</th><th class="r">Fix iter.</th><th class="r">Tokens / succeeded task</th><th class="r">API-eq. cost</th></tr>{et}</table>
<div style="padding:12px 16px;border-top:1px solid var(--border);font-size:12.5px"><span class="pill p-info">Recommendation</span> <span class="muted">For <b style="color:var(--text)">fixer · S</b>, both candidates have similar first-pass rates (82% vs 79%, n=63/37); codex/&lt;small-model&gt; uses 26% fewer tokens.</span></div>
<div style="padding:0 16px 12px;font-size:12.5px"><span class="pill p-unk">unknown</span> <span class="muted">Provider account quota is not exposed by either CLI and is never estimated.</span></div></div></div>"""
    return shell("Token analytics", "Analytics", "<b>Analytics</b> / Tokens &amp; cost", body)

pages = {"dashboard-overview": overview, "dashboard-projects": projects, "dashboard-job-detail": job_detail,
         "dashboard-approvals": approvals, "dashboard-code-review": code_review, "dashboard-token-analytics": analytics}
for name, fn in pages.items():
    (out / f"{name}.html").write_text(fn())
print("ok", list(pages))
