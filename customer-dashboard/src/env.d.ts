/// <reference types="unplugin-icons/types/react" />

declare namespace Cloudflare {
  interface Env {
    HYPERDRIVE: Hyperdrive
    BETTER_AUTH_SECRET: string
    CLI_DEVICE_CODE_PEPPER: string
    LICENSE_CACHE: KVNamespace
  }
}
