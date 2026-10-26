export type Item = { id: number; name: string; tags: string[] };
export type Request = { method: string; path: string; body?: unknown };
export type Response = { status: number; body: unknown };

export function health(): Response {
  return { status: 200, body: { status: "ok" } };
}

export function handleRequest(request: Request): Response {
  if (request.method === "GET" && request.path === "/health") {
    return health();
  }
  if (request.method === "GET" && request.path === "/items") {
    return { status: 200, body: [] };
  }
  return { status: 404, body: { error: "not found" } };
}
