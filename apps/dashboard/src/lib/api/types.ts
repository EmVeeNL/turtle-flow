export type UsageProvenance = 'reported' | 'estimated' | 'unavailable';
export type JobState = 'queued' | 'backlog' | 'discovery' | 'specification' | 'scope_approval' | 'planning' | 'plan_approval' | 'implementation' | 'build_test' | 'review' | 'verification' | 'acceptance_approval' | 'delivery' | 'awaiting_release' | 'done' | 'blocked' | 'cancelled' | 'failed';
export type ApprovalStatus = 'pending' | 'approved' | 'rejected' | 'changes_requested' | 'expired' | 'invalidated' | 'superseded';
export type ApprovalKind = 'scope' | 'plan' | 'budget_increase' | 'scope_change' | 'acceptance' | 'policy_exception' | 'release';
export type TrustLevel = 'trusted' | 'internal' | 'untrusted';
export interface TokenUsage { total: number | null; provenance: UsageProvenance; input?: number | null; cached?: number | null; output?: number | null; reasoning?: number | null; }
export interface TokenEstimate { p10: number; p50: number; p90: number; confidence: 'low' | 'medium' | 'high' | 'unknown'; actual?: number | null; }
export interface Budget { id: string; scope?: 'global' | 'project' | 'job'; settled: number; reserved: number; limit: number; kind: 'hard' | 'soft'; }
export interface Project { id: string; slug: string; key_prefix: string; name: string; description: string; repo_path: string; default_branch: string; default_profile: string; status: 'active' | 'archived'; active_jobs: number; pending_approvals: number; gate_pass_rate: number | null; last_delivery: string | null; health: 'healthy' | 'warning' | 'missing' | 'unknown'; budget: Budget | null; }
export interface Job { id: string; key: string; project_id: string; title: string; description: string; type: 'feature' | 'bugfix' | 'refactor' | 'docs' | 'chore'; profile: string; state: JobState; hold?: 'paused' | 'awaiting_budget' | 'rate_limited' | 'waiting_for_dependencies' | null; holds?: Array<{hold:string;mode?:string;provider?:string}>; priority: 'urgent' | 'high' | 'normal' | 'low'; stage_index: number; provider: string | null; usage: TokenUsage; estimate: TokenEstimate | null; budget?: Budget | null; elapsed: string | null; base_commit?: string | null; head_commit?: string | null; version?: number; }
export interface Task { id: string; job_id: string; ordinal: number; title: string; state: 'pending' | 'ready' | 'running' | 'gating' | 'fixing' | 'succeeded' | 'failed' | 'skipped' | 'superseded'; commit_sha?: string | null; gate_summary?: string | null; }
export interface ArtifactRevision { id: string; job_id: string; kind: string; rev: number; sha256: string; author: string; created_at: string; summary: string | null; }
export interface BoundArtifact { id?: string | null; kind: string; rev: number; sha256: string; content?: string | null; summary?: string | null; }
export interface Approval { id: string; job_id: string; gate_kind: ApprovalKind; subject_hash: string; status: ApprovalStatus; requested_at: string; expires_at?: string | null; binding: { artifacts: BoundArtifact[]; base_commit?: string | null; head_commit?: string | null; diff_sha256?: string | null; target_branch?: string | null; budget_envelope?: number | null; budget_increase?: {budget_id:string;current_tokens:number;requested_tokens:number;stage:JobState} | null; profile?: string; profile_hash?: string; policy_version?: string | number; canonical?: unknown; }; flags: string[]; invalidated_if: string[]; }
export interface AgentRun { id: string; job_id: string; role: string; provider: string; model: string; cli_version: string | null; routing_reason: string | null; status: 'running' | 'succeeded' | 'failed' | 'queued' | 'starting' | 'cancelled' | 'timed_out' | 'abandoned'; duration: string | null; usage: TokenUsage; cost_usd: number | null; }
export interface ResourceVersion { version: number; sha256: string; status: 'ready' | 'extracting' | 'failed'; }
export interface Resource { id: string; title: string; kind: 'file' | 'url_snapshot'; trust: TrustLevel; versions: ResourceVersion[]; attached_to: string[]; size: string; used_in: number; }
export interface Event { id: string; seq: number; occurred_at: string; type: string; actor_kind: 'agent_run' | 'user' | 'system' | 'scheduler'; actor_id: string; job_id?: string | null; payload: { summary: string; detail?: string }; }
export interface Provider { id: string; name: string; version: string | null; model: string | null; status: 'healthy' | 'rate_limited' | 'untested'; running: string | null; quota: string | null; quota_windows?: Array<{window:string;utilization:number;status:string;observed_at:string}>; rate_limit_reset?: string | null; capabilities: Record<string, 'known' | 'unsupported' | 'unknown'>; price_per_million: number | null; }
export interface DashboardData { projects: Project[]; jobs: Job[]; approvals: Approval[]; runs: AgentRun[]; artifacts: ArtifactRevision[]; tasks: Task[]; resources: Resource[]; events: Event[]; providers: Provider[]; budgets?: Budget[]; }
export interface DashboardAdapter {
  me(): Promise<{ readonly: boolean }>;
  overview(): Promise<DashboardData>;
  project(slug: string): Promise<Project | null>;
  job(key: string): Promise<Job | null>;
  approval(id: string): Promise<Approval | null>;
  run(id: string): Promise<AgentRun | null>;
  decideApproval(id: string, decision: 'approve' | 'changes' | 'reject', subject_hash: string, comment: string, acknowledged_flags?: string[]): Promise<{ ok: boolean; error?: string }>;
  requestBudgetApproval(key: string, budget: number): Promise<{ok:boolean;approval_id:string;subject_hash:string}>;
}
