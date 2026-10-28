import type { DashboardAdapter, DashboardData, Approval } from './types';
const sha = (middle: string) => `sha256:${middle}${'c4e119b3a5d7f0e2'.repeat(4).slice(0, 60-middle.length)}${middle.slice(-4)}`;
export const mockData: DashboardData = {
  projects: [
    {id:'p1',slug:'payments-api',key_prefix:'API',name:'Payments API',description:'Payment and billing services',repo_path:'~/code/payments-api',default_branch:'main',default_profile:'standard',status:'active',active_jobs:3,pending_approvals:1,gate_pass_rate:98,last_delivery:'2h ago',health:'healthy',budget:{id:'b1',settled:6400000,reserved:400000,limit:25000000,kind:'hard'}},
    {id:'p2',slug:'web-console',key_prefix:'WEB',name:'Web Console',description:'Customer management interface',repo_path:'~/code/web-console',default_branch:'main',default_profile:'standard',status:'active',active_jobs:2,pending_approvals:1,gate_pass_rate:91,last_delivery:'Yesterday',health:'warning',budget:{id:'b2',settled:3900000,reserved:100000,limit:10000000,kind:'hard'}},
    {id:'p3',slug:'sync-cli',key_prefix:'CLI',name:'Sync CLI',description:'Developer command line tools',repo_path:'~/code/sync-cli',default_branch:'main',default_profile:'strict',status:'active',active_jobs:1,pending_approvals:1,gate_pass_rate:96,last_delivery:'3 days ago',health:'healthy',budget:{id:'b3',settled:4100000,reserved:300000,limit:6000000,kind:'hard'}},
    {id:'p4',slug:'docs-site',key_prefix:'DOC',name:'Docs Site',description:'Public documentation',repo_path:'~/code/docs-site',default_branch:'main',default_profile:'fast',status:'active',active_jobs:1,pending_approvals:0,gate_pass_rate:100,last_delivery:'5 days ago',health:'healthy',budget:{id:'b4',settled:400000,reserved:0,limit:2000000,kind:'soft'}}
  ],
  jobs: [
    {id:'j1',key:'API-42',project_id:'p1',title:'Add per-client rate limiting',description:'Add configurable per-key limits to the public API with 429 responses and Retry-After headers.',type:'feature',profile:'standard',state:'acceptance_approval',priority:'high',stage_index:8,provider:'codex · claude-code',usage:{total:612000,provenance:'reported',input:395000,cached:95000,output:85000,reasoning:37000},estimate:{p10:540000,p50:690000,p90:1050000,confidence:'medium',actual:612000},elapsed:'2h 14m',base_commit:'3f2a9c1',head_commit:'a81d77e'},
    {id:'j2',key:'WEB-17',project_id:'p2',title:'Dark mode for settings pages',description:'Add theme support to account settings.',type:'feature',profile:'standard',state:'scope_approval',priority:'normal',stage_index:2,provider:'claude-code',usage:{total:41000,provenance:'reported'},estimate:{p10:120000,p50:200000,p90:310000,confidence:'medium'},elapsed:'12m'},
    {id:'j3',key:'API-44',project_id:'p1',title:'Idempotency keys for refunds',description:'Prevent duplicate refund requests using an idempotency key.',type:'feature',profile:'standard',state:'implementation',priority:'high',stage_index:5,provider:'codex',usage:{total:288000,provenance:'reported'},estimate:{p10:300000,p50:450000,p90:720000,confidence:'medium',actual:288000},elapsed:'48m'},
    {id:'j4',key:'CLI-8',project_id:'p3',title:'Parallel upload with retries',description:'Make sync resilient to transient upload failures.',type:'feature',profile:'strict',state:'implementation',hold:'awaiting_budget',priority:'high',stage_index:7,provider:'codex',usage:{total:402000,provenance:'reported'},estimate:{p10:140000,p50:200000,p90:260000,confidence:'low',actual:402000},budget:{id:'job-budget-8',scope:'job',settled:1100000,reserved:0,limit:1200000,kind:'hard'},elapsed:'1h 05m'},
    {id:'j5',key:'WEB-12',project_id:'p2',title:'Fix flaky checkout e2e tests',description:'Stabilize checkout integration coverage.',type:'bugfix',profile:'standard',state:'blocked',priority:'urgent',stage_index:8,provider:'claude-code',usage:{total:355000,provenance:'reported'},estimate:{p10:300000,p50:400000,p90:550000,confidence:'medium',actual:355000},elapsed:'1h 32m'},
    {id:'j6',key:'DOC-3',project_id:'p4',title:'Document webhook retries',description:'Explain retry behavior and idempotency.',type:'docs',profile:'fast',state:'specification',priority:'low',stage_index:1,provider:'claude-code',usage:{total:9000,provenance:'reported'},estimate:{p10:30000,p50:45000,p90:75000,confidence:'high',actual:9000},elapsed:'3m'},
    {id:'j7',key:'API-45',project_id:'p1',title:'Upgrade sqlx to 0.8',description:'Update database dependencies and migrations.',type:'chore',profile:'standard',state:'queued',priority:'normal',stage_index:0,provider:'—',usage:{total:null,provenance:'unavailable'},estimate:{p10:100000,p50:160000,p90:250000,confidence:'low'},elapsed:'—'}
  ],
  artifacts: [
    {id:'a1',job_id:'j1',kind:'specification',rev:3,sha256:sha('91af3c07'),author:'agent_run:run-102',created_at:'2026-10-25T08:14:00Z',summary:'Six acceptance criteria for client-tier rate limits'},
    {id:'a2',job_id:'j1',kind:'patch',rev:1,sha256:sha('5d0c41e7'),author:'system',created_at:'2026-10-25T08:26:00Z',summary:'7 files · +295 −1'},
    {id:'a3',job_id:'j1',kind:'verification_report',rev:1,sha256:sha('c4e019d3'),author:'agent_run:run-105',created_at:'2026-10-25T08:31:00Z',summary:'6 of 6 criteria met'},
    {id:'a4',job_id:'j1',kind:'review_report',rev:2,sha256:sha('e19a5b42'),author:'agent_run:run-104',created_at:'2026-10-25T08:30:00Z',summary:'0 blockers · 3 minor findings'}
  ],
  approvals: [
    {id:'ap-42',job_id:'j1',gate_kind:'acceptance',subject_hash:sha('7a4f0c2d'),status:'pending',requested_at:'2026-10-25T08:33:00Z',expires_at:'2026-11-01T08:33:00Z',binding:{artifacts:[],base_commit:'3f2a9c1',head_commit:'a81d77e',budget_envelope:1140000,profile:'standard',policy_version:'v4'},flags:['New dependency governor = "0.6"','Protected path Cargo.lock changed (+38 lines, dependency resolution only)'],invalidated_if:['Any new commit or working-tree change in the job worktree','main moves and the branch is refreshed','Spec, plan or verification report gets a new revision','Policy version changes (protected paths)']},
    {id:'ap-17',job_id:'j2',gate_kind:'scope',subject_hash:sha('92bc414e'),status:'pending',requested_at:'2026-10-25T09:03:00Z',binding:{artifacts:[],budget_envelope:310000,profile:'standard'},flags:[],invalidated_if:['Specification gets a new revision','Profile or budget envelope changes']},
    {id:'ap-8',job_id:'j4',gate_kind:'budget_increase',subject_hash:sha('ba451d22'),status:'pending',requested_at:'2026-10-25T09:09:00Z',binding:{artifacts:[],budget_increase:{budget_id:'job-budget-8',current_tokens:1200000,requested_tokens:1500000,stage:'implementation'},profile:'strict'},flags:[],invalidated_if:['Current budget or held stage changes']}
  ],
  tasks: [
    {id:'rev-p42/T1',job_id:'j1',plan_revision_id:'rev-p42',plan_task_id:'T1',ordinal:1,title:'Add per-key rate limiter',state:'succeeded',depends_on:[],fix_iterations:0,runs:1,commit_sha:'6b3e9af',gates:[{gate:'build',outcome:'passed'},{gate:'test',outcome:'passed'},{gate:'lint',outcome:'passed'}],gate_summary:'build passed · test passed · lint passed'},
    {id:'rev-p42/T2',job_id:'j1',plan_revision_id:'rev-p42',plan_task_id:'T2',ordinal:2,title:'Expose Retry-After and metrics',state:'succeeded',depends_on:['rev-p42/T1'],fix_iterations:1,runs:2,commit_sha:'a81d77e',gates:[{gate:'build',outcome:'passed'},{gate:'test',outcome:'passed'}],gate_summary:'build passed · test passed'},
    {id:'rev-p42/T3',job_id:'j1',plan_revision_id:'rev-p42',plan_task_id:'T3',ordinal:3,title:'Document the limits',state:'succeeded',depends_on:['rev-p42/T2'],fix_iterations:0,runs:1,commit_sha:null,gates:[],gate_summary:null},
    {id:'rev-c8/T1',job_id:'j4',plan_revision_id:'rev-c8',plan_task_id:'T1',ordinal:1,title:'Retry transient upload errors',state:'succeeded',depends_on:[],fix_iterations:0,runs:1,commit_sha:'4c19d02',gates:[{gate:'build',outcome:'passed'},{gate:'test',outcome:'passed'},{gate:'lint',outcome:'passed'}],gate_summary:'build passed · test passed · lint passed'},
    {id:'rev-c8/T2',job_id:'j4',plan_revision_id:'rev-c8',plan_task_id:'T2',ordinal:2,title:'Upload chunks in parallel',state:'fixing',depends_on:['rev-c8/T1'],fix_iterations:2,runs:3,commit_sha:null,gates:[{gate:'build',outcome:'passed'},{gate:'test',outcome:'failed'},{gate:'lint',outcome:'passed'}],gate_summary:'build passed · test failed · lint passed'},
    {id:'rev-c8/T3',job_id:'j4',plan_revision_id:'rev-c8',plan_task_id:'T3',ordinal:3,title:'Report upload progress',state:'pending',depends_on:['rev-c8/T2'],fix_iterations:0,runs:0,commit_sha:null,gates:[],gate_summary:null}
  ],
  runs: [
    {id:'run-105',job_id:'j1',role:'verifier',provider:'claude-code',model:'sonnet-4.5',cli_version:'2.0.31',routing_reason:'Independent provider for verification',status:'succeeded',duration:'8m 42s',usage:{total:97000,provenance:'reported',input:71000,cached:12000,output:11000,reasoning:3000},cost_usd:0.42},
    {id:'run-104',job_id:'j1',role:'reviewer',provider:'claude-code',model:'sonnet-4.5',cli_version:'2.0.31',routing_reason:'Cross-provider review policy',status:'succeeded',duration:'4m 18s',usage:{total:68000,provenance:'reported',input:49000,cached:7000,output:9000,reasoning:3000},cost_usd:0.31},
    {id:'run-103',job_id:'j3',role:'implementer',provider:'codex',model:'gpt-6-sol',cli_version:'0.64.0',routing_reason:'Standard implementation tier',status:'running',duration:'12m 07s',usage:{total:288000,provenance:'reported',input:190000,cached:45000,output:37000,reasoning:16000},cost_usd:null},
    {id:'run-102',job_id:'j1',role:'implementer',provider:'codex',model:'gpt-6-sol',cli_version:'0.64.0',routing_reason:'Standard implementation tier',status:'succeeded',duration:'37m 12s',usage:{total:302000,provenance:'reported'},cost_usd:1.84},
    {id:'run-101',job_id:'j6',role:'spec_writer',provider:'claude-code',model:'sonnet-4.5',cli_version:'2.0.31',routing_reason:'Fast documentation profile',status:'running',duration:'3m 08s',usage:{total:9000,provenance:'reported'},cost_usd:null}
  ],
  resources: [
    {id:'res-1',title:'API contracts v2',kind:'file',trust:'trusted',versions:[{version:3,sha256:sha('0a51b3d9'),status:'ready'}],attached_to:['payments-api'],size:'84 KB',used_in:18},
    {id:'res-2',title:'Rate limit policy',kind:'url_snapshot',trust:'internal',versions:[{version:2,sha256:sha('d94bc811'),status:'ready'}],attached_to:['payments-api','API-42'],size:'12 KB',used_in:4},
    {id:'res-3',title:'Architecture notes',kind:'file',trust:'untrusted',versions:[{version:1,sha256:sha('a303182c'),status:'failed'}],attached_to:['web-console'],size:'2.1 MB',used_in:0}
  ],
  events: [
    {id:'ev-6',seq:6,occurred_at:'2026-10-25T09:15:41Z',type:'run.output',actor_kind:'agent_run',actor_id:'run-103',job_id:'j3',payload:{summary:'Added idempotency key table migration'}},
    {id:'ev-5',seq:5,occurred_at:'2026-10-25T09:15:12Z',type:'gate.passed',actor_kind:'system',actor_id:'gate-runner',job_id:'j3',payload:{summary:'cargo test — 412 passed'}},
    {id:'ev-4',seq:4,occurred_at:'2026-10-25T09:14:58Z',type:'run.output',actor_kind:'agent_run',actor_id:'run-101',job_id:'j6',payload:{summary:'Drafting acceptance criteria (4)'}},
    {id:'ev-3',seq:3,occurred_at:'2026-10-25T09:13:30Z',type:'budget.reservation_denied',actor_kind:'system',actor_id:'ledger',job_id:'j4',payload:{summary:'Reservation denied on job budget'}},
    {id:'ev-2',seq:2,occurred_at:'2026-10-25T09:12:02Z',type:'gate.failed',actor_kind:'system',actor_id:'gate-runner',job_id:'j5',payload:{summary:'checkout.spec.ts — 2 failed (fix 3/3)'}},
    {id:'ev-1',seq:1,occurred_at:'2026-10-25T09:09:15Z',type:'review.complete',actor_kind:'agent_run',actor_id:'run-104',job_id:'j1',payload:{summary:'0 blockers · 1 major (fixed) · 3 minor'}}
  ],
  providers: [
    {id:'claude-code',name:'Claude Code',version:'2.0.31',model:'sonnet-4.5',status:'healthy',running:'1 / 2',quota:null,capabilities:{'Structured output':'known','Session resume':'known','File edit':'known','Web search':'known'},price_per_million:4.2},
    {id:'codex',name:'Codex',version:'0.64.0',model:'gpt-6-sol',status:'rate_limited',running:'2 / 2',quota:null,capabilities:{'Structured output':'known','Session resume':'unknown','File edit':'known','Web search':'known'},price_per_million:null}
  ]
};
mockData.approvals[0]!.binding.artifacts = mockData.artifacts;
export const mockAdapter: DashboardAdapter = {
  async me() { return {readonly:false}; },
  async overview(){ return mockData; },
  async project(slug){ return mockData.projects.find(p=>p.slug===slug) ?? null; },
  async job(key){ const job=mockData.jobs.find(j=>j.key===key); return job ? {...job,tasks:mockData.tasks.filter(t=>t.job_id===job.id)} : null; },
  async approval(id){ return mockData.approvals.find(a=>a.id===id) ?? null; },
  async run(id){ return mockData.runs.find(r=>r.id===id) ?? null; },
  async requestBudgetApproval(key,budget){
    const job=mockData.jobs.find(j=>j.key===key);
    if(!job||job.hold!=='awaiting_budget'||!job.budget||budget<=job.budget.limit)throw new Error('Choose a higher limit for a held job.');
    const approval=mockData.approvals.find(a=>a.job_id===job.id&&a.gate_kind==='budget_increase');
    if(!approval)throw new Error('Budget approval is unavailable.');
    approval.binding.budget_increase={budget_id:job.budget.id,current_tokens:job.budget.limit,requested_tokens:budget,stage:job.state};
    approval.subject_hash=sha(budget.toString(16).padStart(8,'0'));
    return {ok:true,approval_id:approval.id,subject_hash:approval.subject_hash};
  },
  async decideApproval(id, decision, subject_hash, comment){
    const approval: Approval | undefined = mockData.approvals.find(a=>a.id===id);
    if (!approval || approval.status !== 'pending' || approval.subject_hash !== subject_hash) return {ok:false,error:'The approval subject changed. Nothing was approved.'};
    if(approval.gate_kind==='budget_increase'){
      const job=mockData.jobs.find(j=>j.id===approval.job_id),bound=approval.binding.budget_increase;
      if(!job||!bound||job.hold!=='awaiting_budget'||job.state!==bound.stage||job.budget?.id!==bound.budget_id||job.budget.limit!==bound.current_tokens)return {ok:false,error:'The budget approval is stale.'};
    }
    if (decision !== 'approve' && !comment.trim()) return {ok:false,error:'Add a comment to explain your decision.'};
    approval.status = decision === 'approve' ? 'approved' : decision === 'changes' ? 'changes_requested' : 'rejected';
    if(approval.gate_kind==='budget_increase'&&decision==='approve'){
      const job=mockData.jobs.find(j=>j.id===approval.job_id);
      if(job?.budget&&approval.binding.budget_increase){job.budget.limit=approval.binding.budget_increase.requested_tokens;job.hold=null;}
    }
    return {ok:true};
  }
};
