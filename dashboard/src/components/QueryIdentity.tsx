import {useEffect,useState} from 'preact/hooks';
import {mockData} from '../lib/api/mock';
import {formatUsage,formatTokens,shortHash} from '../lib/format';
import type {Job,Project,AgentRun,ArtifactRevision} from '../lib/api/types';
type Kind='project'|'job'|'run'|'artifact';
const defaults:Record<Kind,string>={project:'payments-api',job:'API-42',run:'run-103',artifact:'a1'};
export default function QueryIdentity({kind}:{kind:Kind}){
 const [id,setId]=useState<string|null|undefined>(undefined);
 useEffect(()=>{const value=new URLSearchParams(location.search).get(kind==='project'?'slug':'id');setId(value);const panel=document.querySelector('[data-default-detail]');if(panel)panel.toggleAttribute('hidden',value!==defaults[kind])},[kind]);
 if(id===undefined)return null;
 if(id===null)return <div class="notice warning mb-16">No {kind} ID was supplied. Open an item from the list to inspect it.</div>;
 if(id===defaults[kind])return null;
 const record=kind==='project'?mockData.projects.find(x=>x.slug===id):kind==='job'?mockData.jobs.find(x=>x.key===id):kind==='run'?mockData.runs.find(x=>x.id===id):mockData.artifacts.find(x=>x.id===id);
 if(!record)return <div class="notice danger" role="alert"><strong>{kind[0]?.toUpperCase()}{kind.slice(1)} {id} was not found.</strong><p>Check the ID or return to the list.</p></div>;
 if(kind==='project'){
  const project=record as Project;const jobs=mockData.jobs.filter(job=>job.project_id===project.id);
  return <><div class="page-title"><div><h1>{project.name}</h1><p>{project.repo_path} · {project.default_branch} · {project.default_profile} profile</p></div><span class="pill success">{project.health}</span></div><div class="two-col"><section class="card"><div class="card-head"><h2>Jobs</h2></div>{jobs.map(job=><a class="list-row block" href={`/jobs/view?id=${job.key}`}>{job.key} · {job.title}<small>{job.state.replaceAll('_',' ')}</small></a>)}</section><section class="card"><div class="card-head"><h2>Monthly budget</h2></div><div class="card-body"><strong>{project.budget ? `${formatTokens(project.budget.settled)} / ${formatTokens(project.budget.limit)}` : 'No budget configured'}</strong><p>{project.budget ? `${project.budget.kind} · ${formatTokens(project.budget.reserved)} reserved` : ''}</p></div></section></div></>;
 }
 if(kind==='job'){
  const job=record as Job;const project=mockData.projects.find(p=>p.id===job.project_id);const runs=mockData.runs.filter(run=>run.job_id===job.id);
  return <><div class="page-title"><div><h1>{job.key} · {job.title}</h1><p>{project?.slug} · {job.profile} · {job.state.replaceAll('_',' ')}</p></div><span class={`pill ${job.state==='blocked'?'danger':'ai'}`}>{job.state.replaceAll('_',' ')}</span></div>{job.state==='blocked'&&<div class="notice danger mb-16"><strong>Fix limit reached</strong><p>Open the worktree, resolve checkout tests, then resume the job from the CLI.</p></div>}<div class="two-col"><div class="stack"><section class="card"><div class="card-head"><h2>Job description</h2></div><div class="card-body">{job.description}</div></section><section class="card"><div class="card-head"><h2>Agent runs</h2></div>{runs.length?runs.map(run=><a class="list-row block" href={`/runs/view?id=${run.id}`}>{run.id} · {run.role}<small>{run.provider} · {run.status}</small></a>):<div class="empty">No runs yet</div>}</section></div><section class="card"><div class="card-head"><h2>Usage & estimate</h2></div><div class="card-body"><div class="kpi-value">{formatUsage(job.usage)}</div><p class="muted">{job.estimate ? `≈${formatTokens(job.estimate.p10)}–${formatTokens(job.estimate.p90)} estimated · ${job.estimate.confidence} confidence` : 'Estimate unavailable'}</p></div></section></div></>;
 }
 if(kind==='run'){
  const run=record as AgentRun;const job=mockData.jobs.find(j=>j.id===run.job_id);
  return <><div class="page-title"><div><h1>{run.id} · {run.role}</h1><p><a href={`/jobs/view?id=${job?.key}`}>{job?.key}</a> · {run.provider} / {run.model}</p></div><span class="pill ai">{run.status}</span></div><div class="two-col"><section class="card"><div class="card-head"><h2>Run details</h2></div><table class="key-values"><tbody><tr><th>CLI version</th><td>{run.cli_version}</td></tr><tr><th>Routing reason</th><td>{run.routing_reason}</td></tr><tr><th>Duration</th><td>{run.duration}</td></tr><tr><th>Usage</th><td>{formatUsage(run.usage)} · {run.usage.provenance}</td></tr></tbody></table></section><section class="card"><div class="card-head"><h2>Transcript</h2></div><div class="empty">Open the live run page for the current transcript stream.</div></section></div></>;
 }
 const artifact=record as ArtifactRevision;
 return <><div class="page-title"><div><h1>{artifact.kind.replaceAll('_',' ')} · rev {artifact.rev}</h1><p>{artifact.summary}</p></div></div><section class="card"><div class="card-head"><h2>Immutable revision</h2></div><table class="key-values"><tbody><tr><th>Content hash</th><td><code title={artifact.sha256}>{shortHash(artifact.sha256)}</code></td></tr><tr><th>Author</th><td>{artifact.author}</td></tr><tr><th>Created</th><td>{artifact.created_at}</td></tr></tbody></table></section></>;
}
