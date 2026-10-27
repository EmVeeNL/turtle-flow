import assert from "node:assert/strict";
import { pathToFileURL } from "node:url";
import path from "node:path";

const [project, benchmark] = process.argv.slice(2);
const moduleUrl = pathToFileURL(path.resolve(project, "src/api.ts"));
const api = await import(moduleUrl.href);

switch (benchmark) {
  case "ts-01":
    assert.equal(api.parseLimit(), 20);
    assert.equal(api.parseLimit("0"), 1);
    assert.equal(api.parseLimit("150"), 100);
    assert.equal(api.parseLimit("42"), 42);
    assert.equal(api.parseLimit("2.5"), 20);
    break;
  case "ts-02":
    assert.deepEqual(api.parseTags(" Red,blue, red,,GREEN "), ["red", "blue", "green"]);
    assert.deepEqual(api.parseTags(), []);
    break;
  case "ts-03":
    assert.deepEqual(api.validateItem({ name: "tea", tags: ["drink"] }), { valid: true, errors: [] });
    assert.deepEqual(api.validateItem({ name: "", tags: [1] }), {
      valid: false,
      errors: ["name must be a non-empty string of at most 80 characters", "tags must contain only strings"],
    });
    assert.equal(api.validateItem({ name: "x".repeat(81) }).valid, false);
    break;
  case "ts-04":
    assert.equal(api.parseCursor(), undefined);
    assert.equal(api.parseCursor("0"), 0);
    assert.equal(api.parseCursor("123"), 123);
    for (const value of ["-1", "+2", "01", "1.0", "9007199254740992", "x"]) {
      assert.equal(api.parseCursor(value), undefined, `accepted invalid cursor ${value}`);
    }
    break;
  case "ts-05":
    assert.deepEqual(api.formatError("  NOT_FOUND ", " Missing "), {
      error: { code: "NOT_FOUND", message: "Missing" },
    });
    assert.deepEqual(api.formatError("  ", ""), {
      error: { code: "INTERNAL_ERROR", message: "An unexpected error occurred" },
    });
    break;
  default:
    throw new Error(`unknown TypeScript benchmark: ${benchmark}`);
}

console.log(`${benchmark} acceptance passed`);
