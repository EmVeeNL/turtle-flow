import {useEffect,useState} from 'preact/hooks';
import {liveEvents} from '../lib/api/events';
import {mockData} from '../lib/api/mock';
import type {Event} from '../lib/api/types';
export default function LiveActivity(){
 const [shown,setShown]=useState<Event[]>(mockData.events),[pending,setPending]=useState<Event[]>([]),[status,setStatus]=useState<'connected'|'reconnecting'>('connected');
 useEffect(()=>{const stream=liveEvents(event=>setPending(current=>[...current,event]),setStatus);return()=>stream.stop()},[]);
 return <section class="card"><div class="card-head"><h2>Live agent activity</h2><div class="page-actions">{pending.length>0&&<button class="btn small" onClick={()=>{setShown(current=>[...pending.reverse(),...current].slice(0,20));setPending([])}}>{pending.length} new</button>}<span class={`pill ${status==='connected'?'ai':'warning'}`}>{status==='connected'?'3 running':'Reconnecting'}</span></div></div>{shown.slice(0,8).map((event,index)=>{const job=mockData.jobs.find(item=>item.id===event.job_id);return <div class="event" key={`${event.id}-${index}`}><time>{event.occurred_at.slice(11,19)}</time><span class={`event-dot ${event.type==='gate.passed'?'gate':event.type==='gate.failed'?'error':''}`}></span><div><p>{job?.key} · {event.actor_id} · {event.type}</p><strong>{event.payload.summary}</strong></div></div>})}</section>;
}
