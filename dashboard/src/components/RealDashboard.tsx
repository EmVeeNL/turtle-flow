import { useEffect, useState } from 'preact/hooks';
import type { ComponentChildren } from 'preact';
import { api } from '../lib/api';
import { ApiError, realCommands } from '../lib/api/real';
import { liveEvents } from '../lib/api/events';
import type { Approval, DashboardData, Job } from '../lib/api/types';

type BoundArtifact = { kind: string; rev: number; sha256: string; content: string | null; id: string | null };
type Review = Omit<Approval,'binding'> & { binding: Omit<Approval['binding'],'artifacts'> & { artifacts: BoundArtifact[]; diff_sha256?: string; target_branch?: string; canonical?: unknown } };
const query = (name: string) => typeof location === 'undefined' ? null : new URLSearchParams(location.search).get(name);
const shown = (value: string | number | null | undefined) => value ?? 'unknown';
const tokens = (value: number | null | undefined) => value == null ? 'unknown' : value.toLocaleString();
const jobFor = (data: DashboardData, id: string) => data.jobs.find(job => job.id === id);

export default function RealDashboard({section}:{section:string}) {
  const [data,setData] = useState<DashboardData | null>(null);
  const [review,setReview] = useState<Review | null>(null);
  const [message,setMessage] = useState('');
  const [loading,setLoading] = useState(true);
  const [comment,setComment] = useState('');
  const [checks,setChecks] = useState<string[]>([]);
  const [busy,setBusy] = useState(false);
  const [transcript,setTranscript] = useState<string[]>([]);
  const [nextOffset,setNextOffset] = useState<number|null>(null);
  const path = typeof location === 'undefined' ? '' : location.pathname;
  const id = query('id');
  const slug = query('slug');
  const refresh = () => api.overview().then(setData).catch((error: unknown) => {
    if (error instanceof ApiError && error.status === 401) setMessage('Your session expired. Run turtle dashboard in a terminal to sign in again.');
    else setMessage(error instanceof Error ? error.message : 'The daemon cannot answer.');
  }).finally(() => setLoading(false));
  useEffect(() => {
    void refresh();
    const stream=liveEvents(() => { void refresh(); });
    return () => stream.stop();
  },[]);
  useEffect(() => {
    if (path === '/approvals/view' && id) {
      void api.approval(id).then(value => setReview(value as Review | null)).catch(error => setMessage(error instanceof Error ? error.message : 'Approval unavailable'));
    }
    if (path === '/runs/view' && id) {
      void realCommands.transcript(id).then(page => {setTranscript(page.lines);setNextOffset(page.next_offset)}).catch(error => setMessage(error instanceof Error ? error.message : 'Transcript unavailable'));
    }
  },[path,id]);
  async function decide(decision:'approve'|'changes'|'reject') {
    if (!review) return;
    if (decision === 'approve' && review.flags.some(flag => !checks.includes(flag))) { setMessage('Acknowledge every flag before approving.'); return; }
    if (decision !== 'approve' && !comment.trim()) { setMessage('A comment is required.'); return; }
    setBusy(true);
    try {
      const result=await api.decideApproval(review.id,decision,review.subject_hash,comment,checks);
      if (!result.ok) {setMessage(result.error ?? 'Decision failed'); return;}
      setMessage('Decision recorded.');
      setReview(await api.approval(review.id) as Review | null);
      await refresh();
    } catch(error) { setMessage(error instanceof Error ? error.message : 'Decision failed'); }
    finally {setBusy(false);}
  }
  async function command(job:Job & {version?:number}, action:'start'|'pause'|'resume'|'cancel') {
    if (job.version == null) {setMessage('Job version unavailable. Reload this page.');return;}
    setBusy(true);
    try { await realCommands.job(job.key,action,job.version,crypto.randomUUID()); setMessage(`Command recorded: ${action}.`); await refresh(); }
    catch(error) {setMessage(error instanceof Error ? error.message : 'Command failed');}
    finally {setBusy(false);}
  }
  if (loading) return <div class="notice">Loading {section.toLowerCase()}…</div>;
  if (!data) return <div class="notice danger" role="alert">{message || 'The daemon cannot answer.'}</div>;
  const jobs=data.jobs as (Job & {version?:number})[];
  const approval=data.approvals.find(a=>a.id===id);
  const project=data.projects.find(p=>p.slug===slug);
  const job=jobs.find(j=>j.key===id);
  const run=data.runs.find(r=>r.id===id);
  const artifact=data.artifacts.find(a=>a.id===id);
  const panel = (title:string, body:ComponentChildren) => <section class="card"><div class="card-head"><h2>{title}</h2></div><div class="card-body">{body}</div></section>;
  const empty=(what:string)=><div class="empty">No {what} recorded.</div>;
  return <div class="stack">
    {message&&<div class="notice" role="status">{message}</div>}
    <div class="page-title"><div><h1>{path === '/' ? 'Overview' : section}</h1><p>Local Turtle Flow daemon</p></div></div>
    {path==='/'&&<div class="grid kpis">
      {panel('Active jobs', jobs.filter(j=>!['done','cancelled','failed'].includes(j.state)).length)}
      {panel('Pending approvals', data.approvals.length)}
      {panel('Projects', data.projects.length)}
      {panel('Recent runs', data.runs.length)}
    </div>}
    {(path==='/'||path==='/approvals')&&panel('Pending approvals',data.approvals.length ? data.approvals.map(a => <div class="list-row" key={a.id}><a href={`/approvals/view?id=${a.id}`}>{jobFor(data,a.job_id)?.key ?? a.job_id} · {a.gate_kind}</a><small>Bound to <code>{a.subject_hash}</code></small></div>) : empty('pending approvals'))}
    {(path==='/'||path==='/jobs')&&panel('Jobs',jobs.length ? jobs.map(j=><div class="list-row" key={j.id}><a href={`/jobs/view?id=${j.key}`}><strong>{j.key}</strong> · {j.title}</a><small>{j.state}{j.hold ? ` · ${j.hold}` : ''} · {tokens(j.usage.total)} tokens ({j.usage.provenance})</small></div>) : empty('jobs'))}
    {path==='/projects'&&panel('Projects',data.projects.length ? data.projects.map(p=><div class="list-row" key={p.id}><a href={`/projects/view?slug=${p.slug}`}>{p.name} · {p.slug}</a><small>{p.repo_path} · {p.active_jobs} active jobs · {p.pending_approvals} approvals</small></div>) : empty('projects'))}
    {path==='/projects/view'&&(project ? <>{panel(project.name,<><p>{project.description}</p><p>{project.repo_path} · {project.default_branch}</p><p>Budget: {project.budget ? `${tokens(project.budget.settled)} / ${tokens(project.budget.limit)} tokens` : 'none configured'}</p></>)}{panel('Jobs',jobs.filter(j=>j.project_id===project.id).map(j=><div class="list-row"><a href={`/jobs/view?id=${j.key}`}>{j.key} · {j.title}</a><small>{j.state}</small></div>))}</> : <div class="notice danger">Project not found.</div>)}
    {path==='/jobs/view'&&(job ? <>{panel(`${job.key} · ${job.title}`,<><p>{job.description}</p><p>State: {job.state}{job.hold ? ` · ${job.hold}` : ''}</p><p>Usage: {tokens(job.usage.total)} tokens ({job.usage.provenance})</p><div class="page-actions">{(['start','pause','resume','cancel'] as const).map(action=><button class="btn" disabled={busy} onClick={()=>void command(job,action)}>{action}</button>)}</div></>)}{panel('Runs',data.runs.filter(r=>r.job_id===job.id).map(r=><div class="list-row"><a href={`/runs/view?id=${r.id}`}>{r.id} · {r.role}</a><small>{r.status}</small></div>))}{panel('Approvals',data.approvals.filter(a=>a.job_id===job.id).map(a=><div class="list-row"><a href={`/approvals/view?id=${a.id}`}>{a.gate_kind} · {a.subject_hash}</a></div>))}</> : <div class="notice danger">Job not found.</div>)}
    {path==='/approvals/view'&&(review ? <>{panel('Bound subject',<><p>{jobFor(data,review.job_id)?.key} · {review.gate_kind} · {review.status}</p><p>Subject hash: <code>{review.subject_hash}</code></p><p>Base: {shown(review.binding.base_commit)} · Head: {shown(review.binding.head_commit)}</p><p>Diff hash: {shown(review.binding.diff_sha256)}</p><p>Budget envelope: {tokens(review.binding.budget_envelope)}</p><p>Policy version: {shown(review.binding.policy_version)}</p></>)}{panel('Bound documents',review.binding.artifacts.length ? review.binding.artifacts.map(item=><div class="list-row"><strong>{item.kind} · rev {item.rev}</strong><code>{item.sha256}</code><pre>{item.content ?? 'Content unavailable'}</pre></div>) : empty('bound documents'))}{panel('Decision',<><div class="stack">{review.flags.map(flag=><label><input type="checkbox" checked={checks.includes(flag)} onChange={()=>setChecks(current=>current.includes(flag)?current.filter(f=>f!==flag):[...current,flag])}/> {flag}</label>)}</div><textarea class="textarea" aria-label="Decision comment" placeholder="Comment (required for changes or reject)" value={comment} onInput={event=>setComment(event.currentTarget.value)}/><div class="page-actions decision-bar">{(['approve','changes','reject'] as const).map(action=><button class="btn" disabled={busy||review.status!=='pending'} onClick={()=>void decide(action)}>{action}</button>)}</div><p class="read-only-note notice">Mobile approvals are read-only. Review and decide on a tablet or desktop.</p></>)}</> : <div class="notice danger">{approval ? 'Loading approval…' : 'Approval not found.'}</div>)}
    {path==='/runs'&&panel('Agent runs',data.runs.length ? data.runs.map(r=><div class="list-row"><a href={`/runs/view?id=${r.id}`}>{r.id} · {r.role}</a><small>{r.provider} / {r.model} · {r.status} · {tokens(r.usage.total)} tokens ({r.usage.provenance})</small></div>) : empty('runs'))}
    {path==='/runs/view'&&(run ? <>{panel(`${run.id} · ${run.role}`,<><p>{run.provider} / {run.model} · {run.status}</p><p>Usage: {tokens(run.usage.total)} tokens ({run.usage.provenance})</p><p>Duration: {shown(run.duration)}</p></>)}{panel('Redacted transcript',<><pre>{transcript.join('\n') || 'No transcript stored.'}</pre>{nextOffset!=null&&<button class="btn" onClick={()=>void realCommands.transcript(run.id,nextOffset).then(page=>{setTranscript(lines=>[...lines,...page.lines]);setNextOffset(page.next_offset)})}>Load more</button>}</>)}</> : <div class="notice danger">Run not found.</div>)}
    {path==='/artifacts/view'&&(artifact ? panel(`${artifact.kind} · rev ${artifact.rev}`,<><p>SHA-256: <code>{artifact.sha256}</code></p><p>Created: {artifact.created_at}</p></>) : <div class="notice danger">Artifact not found.</div>)}
    {path==='/providers'&&panel('Providers',data.providers.map(p=><div class="list-row"><strong>{p.name}</strong><small>{p.status} · quota {shown(p.quota)}</small></div>))}
    {path==='/analytics'&&panel('Usage by job',jobs.map(j=><div class="list-row"><a href={`/jobs/view?id=${j.key}`}>{j.key}</a><small>{tokens(j.usage.total)} tokens · {j.usage.provenance}</small></div>))}
    {['/resources','/settings','/license','/jobs/review'].includes(path)&&panel(section,<p>This feature is not available in the live dashboard yet.</p>)}
  </div>;
}
