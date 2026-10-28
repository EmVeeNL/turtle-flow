import type { Task, TaskState } from './api/types';

const done = new Set<TaskState>(['succeeded', 'skipped']);

/** Tasks of the newest plan revision; superseded tasks of earlier revisions are history. */
export function currentTasks(tasks: Task[]): Task[] {
  const live = tasks.filter(t => t.state !== 'superseded');
  const revisions = [...new Set(live.map(t => t.plan_revision_id).filter((r): r is string => !!r))].sort();
  const latest = revisions.at(-1);
  return (latest ? live.filter(t => t.plan_revision_id === latest) : live).sort((a, b) => a.ordinal - b.ordinal);
}

export function taskProgress(tasks: Task[]): string {
  return tasks.length ? `${tasks.filter(t => done.has(t.state)).length}/${tasks.length} done` : 'no tasks';
}

export function taskTone(state: TaskState): 'success' | 'warning' | 'danger' | 'muted' {
  if (done.has(state)) return 'success';
  if (state === 'failed') return 'danger';
  if (state === 'running' || state === 'gating' || state === 'fixing') return 'warning';
  return 'muted';
}

/** Commit, runs, fix iterations and gates in one line; unknown values say so. */
export function taskDetail(task: Task): string {
  const parts = [task.commit_sha ? `commit ${task.commit_sha.slice(0, 8)}` : 'no commit'];
  if (task.runs != null) parts.push(`${task.runs} ${task.runs === 1 ? 'run' : 'runs'}`);
  if (task.fix_iterations) parts.push(`${task.fix_iterations} ${task.fix_iterations === 1 ? 'fix' : 'fixes'}`);
  const gates = task.gates?.length
    ? task.gates.map(g => `${g.gate} ${g.outcome}`).join(' · ')
    : task.gate_summary;
  parts.push(gates ? `gates ${gates}` : 'gates unknown');
  return parts.join(' · ');
}
