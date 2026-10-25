import type { DashboardAdapter, DashboardData, Job, Project, Approval, AgentRun } from './types';
async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`/api/v1${path}`, { credentials: 'same-origin', headers: { 'Accept': 'application/json', ...(init?.headers ?? {}) }, ...init });
  if (!response.ok) throw new Error(`API ${response.status}: ${path}`);
  return response.json() as Promise<T>;
}
export const realAdapter: DashboardAdapter = {
  overview: () => request<DashboardData>('/overview'),
  project: slug => request<Project | null>(`/projects/${encodeURIComponent(slug)}`),
  job: key => request<Job | null>(`/jobs/${encodeURIComponent(key)}`),
  approval: id => request<Approval | null>(`/approvals/${encodeURIComponent(id)}`),
  run: id => request<AgentRun | null>(`/runs/${encodeURIComponent(id)}`),
  decideApproval: (id, decision, subject_hash, comment) => request<{ok:boolean;error?:string}>(`/approvals/${encodeURIComponent(id)}/decisions`, { method:'POST', headers:{'Content-Type':'application/json'}, body:JSON.stringify({decision,subject_hash,comment}) })
};
