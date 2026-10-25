import {useEffect,useState} from 'preact/hooks';
import {liveEvents} from '../lib/api/events';
export default function ConnectionBanner(){
 const [reconnecting,setReconnecting]=useState(false);
 useEffect(()=>{if(import.meta.env.PUBLIC_API_ADAPTER!=='real')return;const stream=liveEvents(()=>{},status=>setReconnecting(status==='reconnecting'));return()=>stream.stop()},[]);
 if(!reconnecting)return null;
 return <div class="notice warning mb-16" role="alert"><strong>Disconnected — retrying</strong><p>Live updates are unavailable. The last loaded data remains visible; check the daemon connection if this persists.</p></div>;
}
