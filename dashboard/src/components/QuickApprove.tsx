import {useState} from 'preact/hooks';
import {api} from '../lib/api';
import {mockData} from '../lib/api/mock';
import {shortHash} from '../lib/format';
export default function QuickApprove({id}:{id:string}){
 const approval=mockData.approvals.find(item=>item.id===id);
 const [open,setOpen]=useState(false),[done,setDone]=useState(false),[error,setError]=useState('');
 if(!approval||approval.gate_kind!=='scope'||approval.binding.artifacts.some(item=>item.kind==='patch'))return null;
 const job=mockData.jobs.find(item=>item.id===approval.job_id);
 const decide=async()=>{const result=await api.decideApproval(id,'approve',approval.subject_hash,'');if(result.ok){setDone(true);setOpen(false)}else setError(result.error??'Approval failed')};
 return <><button class="btn small quick-approve" disabled={done} onClick={()=>setOpen(true)}>{done?'Approved':'Quick approve scope'}</button>{open&&<div class="dialog-backdrop"><div class="dialog" role="dialog" aria-modal="true" aria-label="Confirm scope approval"><h2>Approve scope</h2><p>{job?.key} · {job?.title}</p><p>This approval is bound to <code title={approval.subject_hash}>{shortHash(approval.subject_hash)}</code>. A revised specification invalidates it.</p>{error&&<div class="notice danger" role="alert">{error}</div>}<div class="dialog-actions"><button class="btn" onClick={()=>setOpen(false)}>Cancel</button><button class="btn primary" onClick={decide}>Confirm approval</button></div></div></div>}</>;
}
