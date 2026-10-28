import type { TokenEstimate, TokenUsage, Budget } from './api/types';
export function formatTokens(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value)) return 'unknown';
  if (Math.abs(value) >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  if (Math.abs(value) >= 1_000) return `${Math.round(value / 1_000)}k`;
  return String(value);
}
export function formatUsage(usage: TokenUsage): string { return usage.provenance === 'unavailable' ? 'unknown' : `${usage.provenance === 'estimated' ? '≈' : ''}${formatTokens(usage.total)}`; }
export function formatMoney(value: number | null): string { return value == null ? 'unknown' : `$${value.toFixed(2)}`; }
export function shortHash(hash: string): string {
  const value = hash.replace(/^sha256:/, '');
  return `sha256:${value.slice(0, 4)}…${value.slice(-4)}`;
}
export function estimatePositions(estimate: TokenEstimate): { start: number; width: number; midpoint: number; actual: number | null; scale: number } {
  const scale = Math.max(estimate.p90 * 1.2, estimate.actual ?? 0, 1);
  const percent = (n: number) => Math.min(100, Math.max(0, (n / scale) * 100));
  return {start:percent(estimate.p10),width:percent(estimate.p90)-percent(estimate.p10),midpoint:percent(estimate.p50),actual:estimate.actual == null ? null : percent(estimate.actual),scale};
}
export function budgetPercent(budget: Budget): { settled: number; reserved: number; remaining: number; exceeded: boolean } {
  const limit = Math.max(1, budget.limit);
  const settled = Math.min(100, budget.settled / limit * 100);
  const reserved = Math.min(100-settled, budget.reserved / limit * 100);
  return {settled,reserved,remaining:Math.max(0,100-settled-reserved),exceeded:budget.settled+budget.reserved>budget.limit};
}
export function approvalShortcut(key: string, target: EventTarget | null, modalOpen: boolean): 'approve' | 'changes' | 'reject' | null {
  if (modalOpen || target && typeof HTMLElement !== 'undefined' && target instanceof HTMLElement && (target.isContentEditable || ['INPUT','TEXTAREA','SELECT'].includes(target.tagName))) return null;
  const normalized = key.toLowerCase();
  return normalized === 'a' ? 'approve' : normalized === 'c' ? 'changes' : normalized === 'r' ? 'reject' : null;
}
export function csv(rows: (string | number | null)[][]): string { return rows.map(row=>row.map(value=>`"${String(value ?? '').replaceAll('"','""')}"`).join(',')).join('\r\n'); }
