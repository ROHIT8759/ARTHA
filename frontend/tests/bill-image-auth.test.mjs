import { test, describe, beforeEach, afterEach } from "node:test";
import assert from "node:assert/strict";
import { api, ApiError } from "../lib/api.ts";

// Regression tests for the Phase 10 bill-image flow. Both the upload and the image
// fetch are authenticated with `Authorization: Bearer <token>`; a browser `<img src>`
// cannot send that header, so the page fetches the bytes through these helpers.

const TOKEN_KEY = "artha_token"; // must match TOKEN_KEY in lib/api.ts

let store;
let calls;
let realFetch;

function installBrowserStubs(responder) {
  globalThis.window = {
    localStorage: {
      getItem: (k) => (k in store ? store[k] : null),
      setItem: (k, v) => {
        store[k] = String(v);
      },
      removeItem: (k) => {
        delete store[k];
      },
    },
  };
  globalThis.fetch = async (url, init = {}) => {
    calls.push({ url: String(url), init });
    return responder(String(url), init);
  };
}

beforeEach(() => {
  store = {};
  calls = [];
  realFetch = globalThis.fetch;
});

afterEach(() => {
  delete globalThis.window;
  globalThis.fetch = realFetch;
});

const pngFile = () => new File([new Uint8Array([0x89, 0x50, 0x4e, 0x47])], "bill.png", { type: "image/png" });

describe("uploadBillImage authentication", () => {
  test("sends the session token stored under the app's token key", async () => {
    store[TOKEN_KEY] = "session-token-123";
    installBrowserStubs(() => new Response(JSON.stringify({ image_id: "img-1", sha256: "x", mime: "image/png" }), { status: 200 }));

    const res = await api.uploadBillImage("p-1", pngFile());

    assert.equal(res.image_id, "img-1");
    assert.equal(calls.length, 1);
    assert.match(calls[0].url, /\/api\/purchases\/p-1\/image$/);
    assert.equal(calls[0].init.method, "POST");
    assert.equal(calls[0].init.headers.Authorization, "Bearer session-token-123");
  });

  test("ignores a stray legacy 'token' key (the original bug read that key and sent no header)", async () => {
    store["token"] = "legacy-wrong-key";
    installBrowserStubs(() => new Response(JSON.stringify({ message: "missing Authorization header" }), { status: 401 }));

    await assert.rejects(() => api.uploadBillImage("p-1", pngFile()), /missing Authorization header/);
    assert.equal(calls[0].init.headers.Authorization, undefined);
  });

  test("surfaces the server's error message (e.g. 413 oversize)", async () => {
    store[TOKEN_KEY] = "t";
    installBrowserStubs(() => new Response(JSON.stringify({ message: "File exceeds 10 MiB limit" }), { status: 413 }));

    await assert.rejects(() => api.uploadBillImage("p-1", pngFile()), /File exceeds 10 MiB limit/);
  });
});

describe("fetchBillImageBlob", () => {
  test("sends the Authorization header, bypasses the cache, and returns the bytes as a Blob", async () => {
    store[TOKEN_KEY] = "session-token-123";
    installBrowserStubs(() => new Response(new Uint8Array([1, 2, 3, 4]), { status: 200, headers: { "content-type": "image/png" } }));

    const blob = await api.fetchBillImageBlob("p-9");

    assert.ok(blob instanceof Blob);
    assert.equal(blob.size, 4);
    assert.match(calls[0].url, /\/api\/purchases\/p-9\/image$/);
    assert.equal(calls[0].init.headers.Authorization, "Bearer session-token-123");
    assert.equal(calls[0].init.cache, "no-store");
  });

  test("throws an ApiError carrying the status and server message on failure", async () => {
    store[TOKEN_KEY] = "t";
    installBrowserStubs(() => new Response(JSON.stringify({ message: "Bill image not found" }), { status: 404, statusText: "Not Found" }));

    await assert.rejects(
      () => api.fetchBillImageBlob("p-9"),
      (err) => {
        assert.ok(err instanceof ApiError);
        assert.equal(err.status, 404);
        assert.equal(err.message, "Bill image not found");
        return true;
      }
    );
  });

  test("falls back to the status text when the error body is not JSON", async () => {
    store[TOKEN_KEY] = "t";
    installBrowserStubs(() => new Response("<html>bad gateway</html>", { status: 502, statusText: "Bad Gateway" }));

    await assert.rejects(
      () => api.fetchBillImageBlob("p-9"),
      (err) => err instanceof ApiError && err.status === 502 && err.message === "Bad Gateway"
    );
  });

  test("sends no Authorization header when signed out (server then answers 401)", async () => {
    installBrowserStubs(() => new Response(JSON.stringify({ message: "missing Authorization header" }), { status: 401 }));

    await assert.rejects(() => api.fetchBillImageBlob("p-9"), /missing Authorization header/);
    assert.equal(calls[0].init.headers.Authorization, undefined);
  });
});
