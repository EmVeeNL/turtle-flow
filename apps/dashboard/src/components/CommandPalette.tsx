import { useEffect, useMemo, useRef, useState } from 'preact/hooks';
import { mockData } from '../lib/api/mock';
interface Item { label:string; meta:string; href:string; keywords:string; }
const items:Item[]=[
  ...([['Overview','/'],['Projects','/projects'],['Jobs','/jobs'],['Approvals','/approvals'],['Runs','/runs'],['Analytics','/analytics'],['Providers','/providers'],['Resources','/resources'],['Settings','/settings'],['License','/license']] as [string,string][]).map(([label,href])=>({label,href,meta:'Navigate',keywords:label.toLowerCase()})),
  ...mockData.jobs.map(job=>({label:`${job.key} · ${job.title}`,href:`/jobs/view?id=${job.key}`,meta:'Job',keywords:`${job.key} ${job.title}`.toLowerCase()})),
  ...mockData.approvals.map(approval=>({label:`Review ${mockData.jobs.find(job=>job.id===approval.job_id)?.key} ${approval.gate_kind}`,href:`/approvals/view?id=${approval.id}`,meta:'Approval',keywords:`approve ${approval.gate_kind}`})),
  ...mockData.runs.map(run=>({label:`${run.id} · ${run.role}`,href:`/runs/view?id=${run.id}`,meta:'Run',keywords:`${run.id} ${run.role}`}))
];
export default function CommandPalette(){
 const [open,setOpen]=useState(false),[query,setQuery]=useState(''),[active,setActive]=useState(0);
 const input=useRef<HTMLInputElement>(null);
 useEffect(()=>{
  const key=(event:KeyboardEvent)=>{if(event.key==='Escape')setOpen(false)};
  const bridge=()=>{setOpen(true);(window as Window & {__paletteRequested?:boolean}).__paletteRequested=false};
  if((window as Window & {__paletteRequested?:boolean}).__paletteRequested)bridge();
  const click=(event:MouseEvent)=>{if((event.target as Element).closest('[data-palette-open]'))setOpen(true)};
  window.addEventListener('keydown',key);window.addEventListener('turtle-palette-open',bridge);document.addEventListener('click',click);return()=>{window.removeEventListener('keydown',key);window.removeEventListener('turtle-palette-open',bridge);document.removeEventListener('click',click)};
 },[]);
 useEffect(()=>{if(open){setQuery('');setActive(0);setTimeout(()=>input.current?.focus(),0)}},[open]);
 const results=useMemo(()=>items.filter(item=>item.keywords.includes(query.toLowerCase())).slice(0,9),[query]);
 if(!open)return null;
 return <div class="dialog-backdrop" onMouseDown={event=>{if(event.target===event.currentTarget)setOpen(false)}}><div class="dialog" role="dialog" aria-modal="true" aria-label="Command palette" class="palette-dialog">
  <input ref={input} class="input palette-input" aria-label="Search commands and pages" placeholder="Search jobs, approvals, runs, pages…" value={query} onInput={event=>{setQuery(event.currentTarget.value);setActive(0)}} onKeyDown={event=>{if(event.key==='ArrowDown'){event.preventDefault();setActive(Math.min(results.length-1,active+1))}if(event.key==='ArrowUp'){event.preventDefault();setActive(Math.max(0,active-1))}if(event.key==='Enter'&&results[active])location.href=results[active].href}}/>
  <div class="palette-results">{results.length?results.map((item,index)=><a key={item.href} href={item.href} class={`list-row palette-result ${index===active?'active':''}`} onMouseEnter={()=>setActive(index)}><span>{item.label}</span><span class="muted">{item.meta}</span></a>):<div class="empty">No matching commands</div>}</div><div class="card-foot">↑↓ navigate · Enter open · Esc close</div>
 </div></div>;
}
