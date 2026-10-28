import {afterEach, describe, expect, it, vi} from 'vitest';
import {mockAdapter} from '../src/lib/api/mock';
import {realAdapter} from '../src/lib/api/real';
import type {Task} from '../src/lib/api/types';
import {currentTasks, taskDetail, taskProgress, taskTone} from '../src/lib/tasks';

afterEach(() => vi.unstubAllGlobals());

const task = (over: Partial<Task>): Task => ({id:'r2/T1',job_id:'j',ordinal:1,title:'T',state:'pending',plan_revision_id:'r2',plan_task_id:'T1',...over});

describe('per-task status', () => {
  it('shows the newest plan revision in plan order and counts finished tasks', () => {
    const tasks = currentTasks([
      task({id:'r2/T2',ordinal:2,plan_task_id:'T2',state:'running'}),
      task({id:'r1/T1',plan_revision_id:'r1',state:'succeeded'}),
      task({id:'r1/T2',plan_revision_id:'r1',ordinal:2,state:'superseded'}),
      task({state:'succeeded'})
    ]);
    expect(tasks.map(t => t.id)).toEqual(['r2/T1','r2/T2']);
    expect(taskProgress(tasks)).toBe('1/2 done');
    expect(taskProgress([])).toBe('no tasks');
    expect([taskTone('succeeded'),taskTone('fixing'),taskTone('failed'),taskTone('pending')]).toEqual(['success','warning','danger','muted']);
  });
  it('describes commit, runs, fixes and gates without inventing unknown values', () => {
    expect(taskDetail(task({commit_sha:'0123456789abcdef',runs:2,fix_iterations:1,gates:[{gate:'build',outcome:'passed'},{gate:'test',outcome:'failed'}]})))
      .toBe('commit 01234567 · 2 runs · 1 fix · gates build passed · test failed');
    expect(taskDetail(task({}))).toBe('no commit · gates unknown');
    expect(taskDetail(task({runs:1,gate_summary:'build passed'}))).toBe('no commit · 1 run · gates build passed');
  });
  it('mock job detail carries only that job\'s tasks', async () => {
    const detail = await mockAdapter.job('CLI-8');
    expect(detail?.tasks?.map(t => `${t.plan_task_id}:${t.state}`)).toEqual(['T1:succeeded','T2:fixing','T3:pending']);
    expect(detail?.tasks?.every(t => t.job_id === detail.id)).toBe(true);
    expect(await mockAdapter.job('NOPE-1')).toBeNull();
  });
  it('real job detail requests tasks and returns them as the API sends them', async () => {
    const body = {id:'j1',key:'API-1',tasks:[{id:'rev/T1',job_id:'j1',plan_revision_id:'rev',plan_task_id:'T1',ordinal:1,title:'First',state:'succeeded',depends_on:[],fix_iterations:1,runs:2,commit_sha:'c0ffee',gates:[{gate:'test',outcome:'passed'}],gate_summary:'test passed'}]};
    const fetch = vi.fn().mockResolvedValue(new Response(JSON.stringify(body),{status:200,headers:{'Content-Type':'application/json'}}));
    vi.stubGlobal('fetch',fetch);
    const detail = await realAdapter.job('API-1');
    expect(String(fetch.mock.calls[0]?.[0])).toMatch(/^\/api\/v1\/jobs\/API-1\?include=tasks/);
    expect(detail?.tasks?.[0]).toMatchObject({plan_task_id:'T1',state:'succeeded',runs:2,fix_iterations:1});
    expect(taskDetail(detail!.tasks![0]!)).toBe('commit c0ffee · 2 runs · 1 fix · gates test passed');
  });
});
