import type { DashboardAdapter, DashboardData, Job, Project, Approval, AgentRun } from './types';

export class ApiError extends Error {
  constructor(public status: number, public code: string, message: string) { super(message); this.name = 'ApiError'; }
}
async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  const headers = new Headers(init.headers);
  headers.set('Accept', 'application/json');
  if (init.method && init.method !== 'GET') {
    headers.set('X-Turtle-Request', '1');
    headers.set('Content-Type', 'application/json');
  }
  const response = await fetch(`/api/v1${path}`, { ...init, credentials: 'same-origin', headers });
  if (!response.ok) {
    const body = await response.json().catch(() => null) as {error?: {code?: string; message?: string}} | null;
    throw new ApiError(response.status, body?.error?.code ?? 'http_error', body?.error?.message ?? `API ${response.status}`);
  }
  return response.json() as Promise<T>;
}
async function optional<T>(path: string): Promise<T | null> {
  try { return await request<T>(path); }
  catch (error) { if (error instanceof ApiError && error.status === 404) return null; throw error; }
}
export const realAdapter: DashboardAdapter = {
  me: () => request<{readonly:boolean}>('/me'),
  overview: () => request<DashboardData>('/overview'),
  project: slug => optional<Project>(`/projects/${encodeURIComponent(slug)}`),
  job: key => optional<Job>(`/jobs/${encodeURIComponent(key)}?include=tasks,artifacts,approvals,usage,events`),
  approval: id => optional<Approval>(`/approvals/${encodeURIComponent(id)}`),
  run: id => optional<AgentRun>(`/runs/${encodeURIComponent(id)}`),
  async decideApproval(id, decision, subject_hash, comment, acknowledged_flags = []) {
    try {
      return await request<{ok:boolean;error?:string}>(`/approvals/${encodeURIComponent(id)}/decisions`, { method:'POST', body:JSON.stringify({decision,subject_hash,comment,acknowledged_flags}) });
    } catch (error) {
      if (error instanceof ApiError && [401, 409, 503].includes(error.status)) return {ok:false,error:error.message};
      throw error;
    }
  }
};
export const realCommands = {
  createProject: (input: {path:string;slug?:string;name?:string;key?:string;profile?:string;branch?:string}) =>
    request<{ok:boolean;id:string;slug:string}>('/projects',{method:'POST',body:JSON.stringify(input)}),
  createJob: (slug:string,input:{title:string;description:string;type:string;priority:string;profile?:string;budget?:number;start:boolean}) =>
    request<{ok:boolean;id:string;key:string;version:number;started:boolean}>(`/projects/${encodeURIComponent(slug)}/jobs`,{method:'POST',body:JSON.stringify(input)}),
  setBudget: (input:{scope:'job'|'project'|'global';target?:string;limit:number;soft:boolean}) =>
    request<{ok:boolean;budget:{id:string;settled:number;reserved:number;limit:number;kind:'hard'|'soft'}}>('/budgets',{method:'POST',body:JSON.stringify(input)}),
  job: (key: string, command: 'start' | 'pause' | 'resume' | 'cancel', expected_version: number, idempotency_key: string) =>
    request<{ok:boolean;version:number;replayed:boolean}>(`/jobs/${encodeURIComponent(key)}/commands`, {method:'POST',body:JSON.stringify({command,expected_version,idempotency_key})}),
  transcript: (id: string, offset = 0, limit = 100) =>
    request<{lines:string[];next_offset:number|null;provenance:string}>(`/runs/${encodeURIComponent(id)}/transcript?offset=${offset}&limit=${limit}`),
  artifact: (id:string) => request<{id:string;content:string|null;kind:string;rev:number;sha256:string;created_at:string}>(`/artifacts/${encodeURIComponent(id)}`),
  usage: () => request<{jobs:unknown[]}>(`/analytics/usage`)
};
