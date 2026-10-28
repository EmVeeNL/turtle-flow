import type { ComponentType, SVGProps } from 'react'
import Alert from '~icons/fluent/alert-24-regular'
import ArrowDownload from '~icons/fluent/arrow-download-24-regular'
import ArrowExit from '~icons/fluent/arrow-exit-20-regular'
import ArrowRight from '~icons/fluent/arrow-right-16-regular'
import ArrowUpRight from '~icons/fluent/arrow-up-right-16-regular'
import Cart from '~icons/fluent/cart-24-regular'
import ChatHelp from '~icons/fluent/chat-help-24-regular'
import CheckCircle from '~icons/fluent/checkmark-circle-24-filled'
import ChevronRight from '~icons/fluent/chevron-right-16-regular'
import DocumentCheckmark from '~icons/fluent/document-checkmark-24-regular'
import Fingerprint from '~icons/fluent/fingerprint-24-regular'
import Flow from '~icons/fluent/flow-24-filled'
import History from '~icons/fluent/history-24-regular'
import Home from '~icons/fluent/home-24-regular'
import Info from '~icons/fluent/info-24-regular'
import Key from '~icons/fluent/key-24-regular'
import Leaf from '~icons/fluent/leaf-three-24-regular'
import LockClosed from '~icons/fluent/lock-closed-16-filled'
import MailCheckmark from '~icons/fluent/mail-checkmark-24-regular'
import PhoneLock from '~icons/fluent/phone-lock-24-regular'
import Receipt from '~icons/fluent/receipt-24-regular'
import ShieldLock from '~icons/fluent/shield-lock-24-regular'
import ShieldLockSmall from '~icons/fluent/shield-lock-16-regular'

const iconMap: Record<string, ComponentType<SVGProps<SVGSVGElement>>> = {
  'fluent:alert-24-regular': Alert,
  'fluent:arrow-download-24-regular': ArrowDownload,
  'fluent:arrow-exit-20-regular': ArrowExit,
  'fluent:arrow-right-16-regular': ArrowRight,
  'fluent:arrow-up-right-16-regular': ArrowUpRight,
  'fluent:cart-24-regular': Cart,
  'fluent:chat-help-24-regular': ChatHelp,
  'fluent:checkmark-circle-24-filled': CheckCircle,
  'fluent:chevron-right-16-regular': ChevronRight,
  'fluent:document-checkmark-24-regular': DocumentCheckmark,
  'fluent:fingerprint-24-regular': Fingerprint,
  'fluent:flow-24-filled': Flow,
  'fluent:history-24-regular': History,
  'fluent:home-24-regular': Home,
  'fluent:info-24-regular': Info,
  'fluent:key-24-regular': Key,
  'fluent:leaf-three-24-regular': Leaf,
  'fluent:lock-closed-16-filled': LockClosed,
  'fluent:mail-checkmark-24-regular': MailCheckmark,
  'fluent:phone-lock-24-regular': PhoneLock,
  'fluent:receipt-24-regular': Receipt,
  'fluent:shield-lock-16-regular': ShieldLockSmall,
  'fluent:shield-lock-24-regular': ShieldLock,
}

export function Icon({ icon, width = 20, height = width, className }: { icon: string; width?: number | string; height?: number | string; className?: string }) {
  const SvgIcon = iconMap[icon]
  if (!SvgIcon) return null
  return <SvgIcon width={width} height={height} className={className} aria-hidden="true" focusable="false" />
}
