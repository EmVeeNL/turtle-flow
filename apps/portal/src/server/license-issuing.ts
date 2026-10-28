import { env } from 'cloudflare:workers'

const LICENSE_KEY_ID = 'tf-free-2026-1'

function canonicalJson(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(',')}]`
  if (value && typeof value === 'object') {
    const entries = Object.entries(value).sort(([left], [right]) => (left < right ? -1 : left > right ? 1 : 0))
    return `{${entries.map(([key, entry]) => `${JSON.stringify(key)}:${canonicalJson(entry)}`).join(',')}}`
  }
  return JSON.stringify(value)
}

function decodeBase64(value: string): ArrayBuffer {
  const binary = atob(value)
  const bytes = new Uint8Array(binary.length)
  for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index)
  return bytes.buffer as ArrayBuffer
}

export async function signFreeLicense(payload: Record<string, unknown>) {
  const signingSecret = (env as typeof env & { LICENSE_SIGNING_KEY_PKCS8?: string }).LICENSE_SIGNING_KEY_PKCS8
  if (!signingSecret) throw new Error('License issuance is temporarily unavailable.')

  const privateKey = await crypto.subtle.importKey(
    'pkcs8',
    decodeBase64(signingSecret),
    { name: 'Ed25519' },
    false,
    ['sign'],
  )
  const signature = await crypto.subtle.sign(
    'Ed25519',
    privateKey,
    new TextEncoder().encode(canonicalJson(payload)),
  )
  const bytes = new Uint8Array(signature)
  let binary = ''
  for (const byte of bytes) binary += String.fromCharCode(byte)

  return { keyId: LICENSE_KEY_ID, signature: btoa(binary) }
}
