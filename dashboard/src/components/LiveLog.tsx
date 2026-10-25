import { useEffect, useRef, useState } from 'preact/hooks';
import { liveEvents } from '../lib/api/events';
import { mockData } from '../lib/api/mock';
import type { Event } from '../lib/api/types';
const seed=mockData.events.slice().reverse();
// ANSI SGR sequences are removed before rendering transcript text.
// eslint-disable-next-line no-control-regex
function stripAnsi(value:string){return value.replace(/\u001b\[[0-9;]*m/g,'')}
export default function LiveLog({runId}:{runId?:string}){
 const [lines,setLines]=useState<Event[]>(seed),[announced,setAnnounced]=useState(''),[follow,setFollow]=useState(true),[connection,setConnection]=useState<'connected'|'reconnecting'>('connected'),[newCount,setNewCount]=useState(0);
 const view=useRef<HTMLDivElement>(null),lastAnnounce=useRef(0);
 useEffect(()=>{const stream=liveEvents(event=>{if(runId&&event.job_id!==mockData.runs.find(run=>run.id===runId)?.job_id)return;setLines(current=>[...current.slice(-499),event]);if(!follow)setNewCount(n=>n+1);if(Date.now()-lastAnnounce.current>4000){lastAnnounce.current=Date.now();setAnnounced(event.payload.summary)}},setConnection);return()=>stream.stop()},[runId,follow]);
 useEffect(()=>{if(follow&&view.current)view.current.scrollTop=view.current.scrollHeight},[lines,follow]);
 const [scrollTop,setScrollTop]=useState(0);const rowHeight=30;const height=430;const start=Math.max(0,Math.floor(scrollTop/rowHeight)-5);const visible=lines.slice(start,start+Math.ceil(height/rowHeight)+10);
 return <div><div class="card-head"><h2>Live transcript</h2><div class="page-actions"><span class={`pill ${connection==='connected'?'success':'warning'}`}>{connection==='connected'?'Streaming':'Reconnecting'}</span><button class="btn small" onClick={()=>{setFollow(v=>!v);setNewCount(0)}}>{follow?'Pause autoscroll':'Follow stream'}</button>{newCount>0&&<button class="btn small primary" onClick={()=>{setFollow(true);setNewCount(0)}}>{newCount} new</button>}</div></div><div ref={view} class="log-viewport" onScroll={event=>setScrollTop(event.currentTarget.scrollTop)} aria-label="Agent transcript" tabIndex={0} role="region"><div><svg width="1" height={start*rowHeight} aria-hidden="true" class="log-spacer" />{visible.map((line,index)=><div class="log-line" key={`${line.id}-${start+index}`}><time>{line.occurred_at.slice(11,19)}</time><span class="kind">{line.type}</span><code>{stripAnsi(line.payload.summary)}</code></div>)}<svg width="1" height={Math.max(0,lines.length-start-visible.length)*rowHeight} aria-hidden="true" class="log-spacer" /></div></div><span class="sr-only" aria-live="polite">{announced}</span></div>;
}
