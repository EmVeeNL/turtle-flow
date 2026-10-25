import { mockAdapter } from './mock';
import { realAdapter } from './real';
export const api = import.meta.env.PUBLIC_API_ADAPTER === 'real' ? realAdapter : mockAdapter;
export * from './types';
