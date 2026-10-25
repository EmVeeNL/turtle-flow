import {describe,it,expect,vi} from 'vitest';
import {formatTokens,formatUsage,shortHash,estimatePositions,budgetPercent,approvalShortcut} from '../src/lib/format';
import {ReconnectingEventStream,type StreamTransport} from '../src/lib/api/sse';
import type {Event} from '../src/lib/api/types';
describe('formatting',()=>{
 it('formats measured, estimated and unknown distinctly',()=>{expect(formatTokens(21300000)).toBe('21.3M');expect(formatTokens(null)).toBe('unknown');expect(formatUsage({total:140000,provenance:'estimated'})).toBe('≈140k');expect(formatUsage({total:140000,provenance:'reported'})).toBe('140k');expect(formatUsage({total:null,provenance:'unavailable'})).toBe('unknown')});
 it('truncates hash without changing the source',()=>{const hash=`sha256:${'a'.repeat(60)}1234`;expect(shortHash(hash)).toBe('sha256:aaaa…1234');expect(hash).toHaveLength(71)});
});
describe('estimate and budget bars',()=>{
 it('positions P10, P50, P90 and measured actual',()=>{const p=estimatePositions({p10:100,p50:200,p90:300,actual:250,confidence:'medium'});expect(p.start).toBeLessThan(p.midpoint);expect(p.midpoint).toBeLessThan(p.actual!);expect(p.actual!).toBeLessThan(p.start+p.width)});
 it('distinguishes unknown actual from zero',()=>expect(estimatePositions({p10:100,p50:200,p90:300,confidence:'low'}).actual).toBeNull());
 it('caps overspent budget display',()=>expect(budgetPercent({id:'b',settled:90,reserved:20,limit:100,kind:'hard'})).toEqual({settled:90,reserved:10,remaining:0,exceeded:true}));
});
describe('approval shortcuts',()=>it('maps A C R only outside dialogs',()=>{expect(approvalShortcut('A',null,false)).toBe('approve');expect(approvalShortcut('c',null,false)).toBe('changes');expect(approvalShortcut('r',null,false)).toBe('reject');expect(approvalShortcut('a',null,true)).toBeNull();expect(approvalShortcut('x',null,false)).toBeNull()}));
describe('SSE reconnect',()=>it('reconnects with last ID and ignores replay',()=>{
 vi.useFakeTimers();const requests:(string|null)[]=[];let emit:((e:Event)=>void)|undefined;let fail:(()=>void)|undefined;let closed=0;
 const transport:StreamTransport={connect(_url,last,onEvent,onError){requests.push(last);emit=onEvent;fail=onError;return{close:()=>closed++}}};
 const seen:string[]=[];const stream=new ReconnectingEventStream('/api/v1/events',transport,event=>seen.push(event.id));stream.start();
 const event={id:'ev-1',seq:1,occurred_at:'',type:'run.output',actor_kind:'system',actor_id:'test',payload:{summary:'hello'}} as Event;
 emit?.(event);fail?.();vi.advanceTimersByTime(1000);expect(requests).toEqual([null,'ev-1']);emit?.(event);expect(seen).toEqual(['ev-1']);stream.stop();expect(closed).toBeGreaterThanOrEqual(2);vi.useRealTimers();
}));
