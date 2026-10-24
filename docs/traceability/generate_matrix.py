# Regenerates REQUIREMENTS-MATRIX.md from PRD.md tables. Usage: python3 generate_matrix.py <package-root>
import re, sys, pathlib
root = pathlib.Path(sys.argv[1])
prd = (root/"PRD.md").read_text()
rows = []
for line in prd.splitlines():
    m = re.match(r"\|\s*((?:FR|NFR|SEC|UX|LIC)-\d{3})\s*\|(.*)\|\s*$", line)
    if not m: continue
    rid = m.group(1); cells = [c.strip() for c in m.group(2).split("|")]
    # find priority cell: last cell containing Must/Should/Could/Won't
    prio = next((c for c in reversed(cells) if re.search(r"Must|Should|Could|Won't", c)), "")
    text = cells[0] if not rid.startswith(("NFR","UX-02","UX-03")) else cells[1] if rid.startswith("NFR") else cells[1]
    if rid.startswith("UX-0") and not rid.startswith(("UX-02","UX-03")): text = cells[0]
    if rid.startswith(("UX-02","UX-03")): text = f"{cells[0]}: {cells[1]}"
    if rid.startswith("LIC"): text = cells[0]
    rows.append((rid, text, prio))

A = lambda *n: ", ".join(f"[ADR-{x:03d}](../adr/{f})" for x,f in n)
ADR = {1:"ADR-001-local-first-modular-monolith.md",2:"ADR-002-rust-technology-stack.md",3:"ADR-003-embedded-webserver-dashboard.md",4:"ADR-004-sqlite-persistence-event-logging.md",5:"ADR-005-workflow-state-machine.md",6:"ADR-006-supervised-autonomy-approvals.md",7:"ADR-007-provider-adapter-architecture.md",8:"ADR-008-token-intelligence-budgets.md",9:"ADR-009-git-worktree-isolation.md",10:"ADR-010-project-memory-context-retrieval.md",11:"ADR-011-job-scheduling-concurrency.md",12:"ADR-012-agent-execution-security.md",13:"ADR-013-resource-artifact-management.md",14:"ADR-014-licensing-architecture.md",15:"ADR-015-future-server-deployment.md",16:"ADR-016-observability-auditability.md",17:"ADR-017-recovery-idempotency-crash-safety.md",18:"ADR-018-model-routing-and-evaluation.md"}
def adr(*ns): return ", ".join(f"[ADR-{n:03d}](../adr/{ADR[n]})" for n in ns)
def doc(d): return f"[{d}](../{d})"

def rule(rid):
    p, n = rid.split("-"); n = int(n)
    if p == "FR":
        if n <= 12: return adr(1,4), doc("DOMAIN-MODEL.md"), "E02, E12", "TST-06, TST-15"
        if n <= 45:
            a = (5,6) if n in (25,32,33,39,41,42) else (5,11) if n in (36,37,40) else (5,)
            return adr(*a), doc("WORKFLOW.md"), "E03" + (", E10" if 6 in a else "") + (", E04" if 11 in a else ""), "TST-02, TST-03, TST-15"
        if n <= 66:
            a = (7,18) if n in (57,58,59,60,64) else (7,12) if n in (65,) else (7,10) if n==61 else (7,)
            t = "TST-20" if n==64 else "TST-09, TST-10, TST-11, TST-15"
            return adr(*a), doc("AGENT-PROTOCOL.md"), "E06" + (", E07" if n==61 else ""), t
        if n <= 86: return adr(8,18) if n in (74,75,76,80) else adr(8), doc("TOKEN-INTELLIGENCE.md"), "E11", "TST-04, TST-05, TST-15"
        if n <= 102: return adr(9,17) if n in (98,99) else adr(9), doc("GIT-STRATEGY.md"), "E08", "TST-07, TST-12, TST-15"
        if n <= 118: return adr(13,10), doc("DOMAIN-MODEL.md"), "E07", "TST-06, TST-13, TST-15"
        if n <= 128: return adr(10), doc("ADR-010 / TOKEN-INTELLIGENCE.md") if False else doc("TOKEN-INTELLIGENCE.md"), "post-MVP (0.4+)" , "TST-05, TST-20"
        if n <= 140: return adr(16,3), doc("ARCHITECTURE.md"), "E12, E13", "TST-14, TST-15, TST-18"
        return adr(5,6), doc("WORKFLOW.md"), "post-MVP (0.5)", "TST-02, TST-15"
    if p == "NFR":
        m = {1:((1,2),"ARCHITECTURE.md","E01, E14","Release CI smoke"),2:((1,4),"ARCHITECTURE.md","E04","TST-19"),3:((3,),"DASHBOARD-UX.md","E12, E13","TST-19"),
             4:((4,17),"WORKFLOW.md","E02, E04","TST-12"),5:((17,),"WORKFLOW.md","E04, E08","TST-12"),6:((1,),"ARCHITECTURE.md","E04","TST-19"),
             7:((11,),"WORKFLOW.md","E04","TST-19, TST-03"),8:((7,),"AGENT-PROTOCOL.md","E06","TST-09 (fuzz)"),9:((4,),"DATA-MODEL.md","E02","TST-06"),
             10:((1,),"ARCHITECTURE.md","E01","TST-15 (offline)"),11:((16,),"ARCHITECTURE.md","E01","TST-14"),12:((3,),"DASHBOARD-UX.md","E13","TST-18"),
             13:((5,),"TESTING-STRATEGY.md","all","CI coverage"),14:((13,),"DATA-MODEL.md","E12","TST-06"),15:((2,),"ARCHITECTURE.md","E01","CI"),
             16:((4,),"DATA-MODEL.md","E02, E13","TST-19"),17:((4,),"DATA-MODEL.md","E02","TST-06"),18:((4,),"DOMAIN-MODEL.md","E01","TST-01")}[n]
        return adr(*m[0]), doc(m[1]), m[2], m[3]
    if p == "SEC":
        m = {1:(3,12),2:(3,12),3:(12,13),4:(12,),5:(12,7),6:(12,7),7:(12,),8:(6,),9:(16,4),10:(12,10),11:(6,9),12:(12,),13:(12,),14:(9,12),15:(12,5),16:(12,),17:(14,),18:(12,),19:(12,),20:(2,)}[n]
        ep = {1:"E13",2:"E13",3:"E07",4:"E05",5:"E06",6:"E06",7:"E05",8:"E10",9:"E02",10:"E07",11:"E09",12:"E06",13:"E01",14:"E08",15:"E10",16:"E09",17:"post-MVP (1.0)",18:"post-MVP (0.5)",19:"post-MVP",20:"E01, E14"}[n]
        return adr(*m), doc("SECURITY.md"), ep, "TST-13" if n not in (17,18,19,20) else ("TST-01" if n==17 else "CI (cargo-deny/audit)" if n==20 else "—")
    if p == "UX": return adr(3), doc("DASHBOARD-UX.md"), "E13" + (", E12" if n in (7,11,12) else ""), "TST-17, TST-18"
    if p == "LIC": return adr(14), doc("LICENSING.md"), "E01 (abstraction)" if n==2 else "post-MVP (1.0)" if n==3 else "E01, E13 (principle)", "TST-01"
out = ["# Requirements Traceability Matrix","",
"| Field | Value |","|---|---|","| Document | traceability/REQUIREMENTS-MATRIX.md |","| Status | Generated from [PRD.md](../PRD.md) requirement tables (do not edit rows by hand; regenerate when the PRD changes) |",
"| Related | [MVP.md §4](../MVP.md#4-epics) (epics), [TESTING-STRATEGY.md §2](../TESTING-STRATEGY.md#2-test-pyramid-and-suites) (test suites) |","",
"Columns: requirement → MVP priority → deciding ADR(s) → primary design document → MVP epic → verifying test suite(s). Requirements prioritized *Won't (MVP)* are listed for completeness and mapped to their planned release.",""]
from collections import Counter
cnt = Counter()
for grp, title in [("FR","Functional requirements"),("NFR","Non-functional requirements"),("SEC","Security requirements"),("UX","User experience requirements"),("LIC","Licensing requirements")]:
    out += [f"## {title}","","| ID | Requirement | MVP | ADR | Design doc | Epic | Tests |","|---|---|---|---|---|---|---|"]
    for rid, text, prio in rows:
        if rid.split("-")[0] != grp: continue
        a,d,e,t = rule(rid)
        text = re.sub(r"\[([^\]]+)\]\([^)]+\)", r"\1", re.sub(r"\s+"," ",text))
        if len(text) > 140: text = text[:137].rstrip()+"…"
        out.append(f"| {rid} | {text.replace('|','/')} | {prio} | {a} | {d} | {e} | {t} |")
        key = "Must" if prio.startswith("Must") else "Should" if prio.startswith("Should") else "Could" if prio.startswith("Could") else "Won't"
        cnt[(grp,key)] += 1
    out.append("")
out += ["## Summary","","| Group | Must | Should | Could | Won't (MVP) |","|---|---|---|---|---|"]
for g in ["FR","NFR","SEC","UX","LIC"]:
    out.append(f"| {g} | {cnt[(g,'Must')]} | {cnt[(g,'Should')]} | {cnt[(g,'Could')]} | {cnt[(g,'Won' + chr(39) + 't')]} |")
out += ["","Priority is classified by the first priority word in the PRD cell (e.g. \"Must (config) / Should (detection)\" counts as Must for the mandatory part).",""]
out += ["## Acceptance criteria coverage","","| AC | Verified by |","|---|---|",
"| AC-01 End-to-end | TST-16 (live E2E, 3 reference repos) |","| AC-02 Approval integrity | TST-13 (SEC-008 bypass suite), TST-03 |",
"| AC-03 Recovery | TST-12 (25 boundaries + kill harness) |","| AC-04 Budgets | TST-04, E2E-06 (TST-15) |","| AC-05 Token honesty | TST-09, TST-17 |",
"| AC-06 Gates | E2E-02/03 (TST-15) |","| AC-07 Isolation | E2E-08 (TST-15), TST-07 |","| AC-08 Security essentials | TST-13 |","| AC-09 Observability | TST-15, TST-06 |","| AC-10 Onboarding | Usability study (MS-23) |",""]
(root/"traceability"/"REQUIREMENTS-MATRIX.md").write_text("\n".join(out))
print(len(rows), dict(cnt))
