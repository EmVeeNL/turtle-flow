import type { Event } from './types';
export interface StreamTransport { connect(url: string, lastEventId: string | null, onEvent: (event: Event) => void, onError: () => void): {close(): void}; }
export class ReconnectingEventStream {
  private connection: {close():void} | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private attempts = 0;
  private lastEventId: string | null = null;
  private stopped = false;
  constructor(private url: string, private transport: StreamTransport, private onEvent: (event: Event) => void, private onStatus: (status:'connected'|'reconnecting')=>void = ()=>{}) {}
  start(): void { this.stopped=false; this.open(); }
  private open(): void {
    this.connection = this.transport.connect(this.url,this.lastEventId,event=>{
      if (this.stopped || event.id === this.lastEventId) return;
      this.lastEventId=event.id;this.attempts=0;this.onStatus('connected');this.onEvent(event);
    },()=>{
      if (this.stopped) return;
      this.connection?.close();this.onStatus('reconnecting');
      const delay = Math.min(30000, 1000 * 2 ** this.attempts++);
      this.timer=setTimeout(()=>this.open(),delay);
    });
  }
  stop(): void { this.stopped=true;this.connection?.close();if(this.timer)clearTimeout(this.timer); }
  get cursor(): string | null { return this.lastEventId; }
}
export const browserTransport: StreamTransport = {
  connect(url,lastEventId,onEvent,onError){
    const stream = new EventSource(lastEventId ? `${url}${url.includes('?')?'&':'?'}lastEventId=${encodeURIComponent(lastEventId)}` : url);
    stream.onmessage = message => { try { onEvent(JSON.parse(message.data) as Event); } catch { /* malformed event ignored */ } };
    stream.onerror = onError;
    return {close:()=>stream.close()};
  }
};
export function mockTransport(events: Event[], interval=2500): StreamTransport {
  return { connect(_url,lastEventId,onEvent){
    let cursor = Math.max(0,events.findIndex(event=>event.id===lastEventId)+1);
    const timer=setInterval(()=>{ const event=events[cursor++ % events.length];if(event)onEvent(event); },interval);
    return {close:()=>clearInterval(timer)};
  } };
}
