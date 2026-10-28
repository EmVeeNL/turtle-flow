import {afterEach, describe, expect, it, vi} from 'vitest';
import {ApiError, realAdapter} from '../src/lib/api/real';

afterEach(() => vi.unstubAllGlobals());

describe('real dashboard adapter', () => {
  it('reads the server-enforced session scope', async () => {
    const fetch=vi.fn().mockResolvedValue(new Response(JSON.stringify({readonly:true}),{status:200,headers:{'Content-Type':'application/json'}}));
    vi.stubGlobal('fetch',fetch);
    expect(await realAdapter.me()).toEqual({readonly:true});
    expect(fetch.mock.calls[0]?.[0]).toBe('/api/v1/me');
  });
  it('uses same-origin credentials and sends the approval binding with CSRF header', async () => {
    const fetch = vi.fn().mockResolvedValue(new Response(JSON.stringify({ok:true}), {status:200,headers:{'Content-Type':'application/json'}}));
    vi.stubGlobal('fetch',fetch);
    const result=await realAdapter.decideApproval('approval 1','approve','sha256:abc','reviewed',['protected_path:Cargo.lock']);
    expect(result.ok).toBe(true);
    const [url,init]=fetch.mock.calls[0] as [string,RequestInit];
    expect(url).toBe('/api/v1/approvals/approval%201/decisions');
    expect(init.credentials).toBe('same-origin');
    expect(new Headers(init.headers).get('X-Turtle-Request')).toBe('1');
    expect(JSON.parse(init.body as string)).toEqual({
      decision:'approve',subject_hash:'sha256:abc',comment:'reviewed',
      acknowledged_flags:['protected_path:Cargo.lock']
    });
  });
  it('requests an exact budget increase for a held job', async () => {
    const fetch=vi.fn().mockResolvedValue(new Response(JSON.stringify({ok:true,approval_id:'a1',subject_hash:'abc'}),{status:200}));
    vi.stubGlobal('fetch',fetch);
    expect(await realAdapter.requestBudgetApproval('CLI-8',1500000)).toMatchObject({approval_id:'a1'});
    const [url,init]=fetch.mock.calls[0] as [string,RequestInit];
    expect(url).toBe('/api/v1/jobs/CLI-8/budget-approvals');
    expect(JSON.parse(init.body as string)).toEqual({budget:1500000});
  });
  it('returns null for missing detail and reports stale decisions', async () => {
    const fetch=vi.fn()
      .mockResolvedValueOnce(new Response(JSON.stringify({error:{code:'not_found',message:'missing'}}),{status:404}))
      .mockResolvedValueOnce(new Response(JSON.stringify({error:{code:'conflict',message:'binding changed'}}),{status:409}));
    vi.stubGlobal('fetch',fetch);
    expect(await realAdapter.job('API-999')).toBeNull();
    expect(await realAdapter.decideApproval('a','approve','old','')).toEqual({ok:false,error:'binding changed'});
  });
  it('surfaces daemon and authentication failures with stable codes', async () => {
    const fetch=vi.fn()
      .mockResolvedValueOnce(new Response(JSON.stringify({error:{code:'unauthorized',message:'login required'}}),{status:401}))
      .mockResolvedValueOnce(new Response(JSON.stringify({error:{code:'unavailable',message:'daemon cannot answer'}}),{status:503}));
    vi.stubGlobal('fetch',fetch);
    await expect(realAdapter.overview()).rejects.toMatchObject({status:401,code:'unauthorized'});
    await expect(realAdapter.overview()).rejects.toMatchObject({status:503,code:'unavailable'});
    expect(ApiError.name).toBe('ApiError');
  });
});
