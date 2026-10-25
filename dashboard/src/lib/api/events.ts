import { mockData } from './mock';
import { browserTransport, mockTransport, ReconnectingEventStream } from './sse';
import type { Event } from './types';
export function liveEvents(onEvent:(event:Event)=>void,onStatus?:(status:'connected'|'reconnecting')=>void):ReconnectingEventStream {
  const mock = import.meta.env.PUBLIC_API_ADAPTER !== 'real';
  const stream=new ReconnectingEventStream('/api/v1/events',mock ? mockTransport([...mockData.events].reverse()) : browserTransport,onEvent,onStatus);
  stream.start();
  return stream;
}
