export type UsageProvenance = 'reported' | 'estimated' | 'unavailable';
export type JobState = 'queued' | 'specification' | 'scope_approval' | 'planning' | 'implementation' | 'review' | 'verification' | 'acceptance_approval' | 'delivery' | 'done' | 'blocked';
export type ApprovalStatus = 'pending' | 'approved' | 'rejected' | 'changes_requested' | 'expired' | 'invalidated';
export type ApprovalKind = 'scope' | 'plan' | 'budget_increase' | 'scope_change' | 'acceptance' | 'policy_exception' | 'release';
export type TrustLevel = 'trusted' | 'internal' | 'untrusted';
export interface TokenUsage { total: number | null; provenance: UsageProvenance; input?: number; cached?: number; output?: number; reasoning?: number; }
export interface TokenEstimate { p10: number; p50: number; p90: number; confidence: 'low' | 'medium' | 'high'; actual?: number | null; }
export interface Budget { id: string; settled: number; reserved: number; limit: number; kind: 'hard' | 'soft'; }
export interface Project { id: string; slug: string; key_prefix: string; name: string; description: string; repo_path: string; default_branch: string; default_profile: string; status: 'active' | 'archived'; active_jobs: number; pending_approvals: number; gate_pass_rate: number; last_delivery: string; health: 'healthy' | 'warning' | 'missing'; budget: Budget; }
export interface Job { id: string; key: string; project_id: string; title: string; description: string; type: 'feature' | 'bugfix' | 'refactor' | 'docs' | 'chore'; profile: string; state: JobState; hold?: 'paused' | 'awaiting_budget' | 'rate_limited'; priority: 'urgent' | 'high' | 'normal' | 'low'; stage_index: number; provider: string; usage: TokenUsage; estimate: TokenEstimate; elapsed: string; base_commit?: string; head_commit?: string; }
export interface Task { id: string; job_id: string; ordinal: number; title: string; state: 'pending' | 'running' | 'gating' | 'fixing' | 'succeeded' | 'failed'; commit_sha?: string; gate_summary?: string; }
export interface ArtifactRevision { id: string; job_id: string; kind: string; rev: number; sha256: string; author: string; created_at: string; summary: string; }
export interface Approval { id: string; job_id: string; gate_kind: ApprovalKind; subject_hash: string; status: ApprovalStatus; requested_at: string; expires_at?: string; binding: { artifacts: ArtifactRevision[]; base_commit?: string; head_commit?: string; budget_envelope?: number; profile?: string; policy_version?: string; }; flags: string[]; invalidated_if: string[]; }
export interface AgentRun { id: string; job_id: string; role: string; provider: string; model: string; cli_version: string; routing_reason: string; status: 'running' | 'succeeded' | 'failed' | 'queued'; duration: string; usage: TokenUsage; cost_usd: number | null; }
export interface ResourceVersion { version: number; sha256: string; status: 'ready' | 'extracting' | 'failed'; }
export interface Resource { id: string; title: string; kind: 'file' | 'url_snapshot'; trust: TrustLevel; versions: ResourceVersion[]; attached_to: string[]; size: string; used_in: number; }
export interface Event { id: string; seq: number; occurred_at: string; type: string; actor_kind: 'agent_run' | 'user' | 'system'; actor_id: string; job_id?: string; payload: { summary: string; detail?: string }; }
export interface Provider { id: string; name: string; version: string; model: string; status: 'healthy' | 'rate_limited' | 'untested'; running: string; quota: string | null; capabilities: Record<string, 'known' | 'unsupported' | 'unknown'>; price_per_million: number | null; }
export interface DashboardData { projects: Project[]; jobs: Job[]; approvals: Approval[]; runs: AgentRun[]; artifacts: ArtifactRevision[]; tasks: Task[]; resources: Resource[]; events: Event[]; providers: Provider[]; }
export interface DashboardAdapter {
  overview(): Promise<DashboardData>;
  project(slug: string): Promise<Project | null>;
  job(key: string): Promise<Job | null>;
  approval(id: string): Promise<Approval | null>;
  run(id: string): Promise<AgentRun | null>;
  decideApproval(id: string, decision: 'approve' | 'changes' | 'reject', subject_hash: string, comment: string): Promise<{ ok: boolean; error?: string }>;
}
