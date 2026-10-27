import assert from "node:assert/strict";
import { handleRequest } from "../src/api.ts";

assert.deepEqual(handleRequest({ method: "GET", path: "/health" }), {
  status: 200,
  body: { status: "ok" },
});
