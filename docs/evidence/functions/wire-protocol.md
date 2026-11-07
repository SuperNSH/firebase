# Functions callable wire protocol — verified by reading

This area was built under the battery constraint: no compile, build,
test, or gate command. The wire protocol below is verified by reading
the four sources listed, quoting the exact lines. Each claim in
`docs/status/functions.md` traces to one of these quotes.

## 1. The client SDK: `@firebase/functions` 0.14.0

File: `~/.bun/install/cache/@firebase/functions@0.14.0@@@1/dist/esm/index.esm.js`
(the compiled mirror of the installed package; `dist/src/*.d.ts` confirms
the module layout). All line numbers are from that file.

### The URL (`FunctionsService._url`, lines 479-489)

```js
_url(name) {
    const projectId = this.app.options.projectId;
    if (this.emulatorOrigin !== null) {
        const origin = this.emulatorOrigin;
        return `${origin}/${projectId}/${this.region}/${name}`;
    }
    if (this.customDomain !== null) {
        return `${this.customDomain}/${name}`;
    }
    return `https://${this.region}-${projectId}.cloudfunctions.net/${name}`;
}
```

`DEFAULT_REGION = 'us-central1'` (line 413). The emulator origin is set
by `connectFunctionsEmulator` as `http://${host}:${port}` (line 502).

### The request (`postJSON` + `callAtURL`, lines 550-630)

```js
headers['Content-Type'] = 'application/json';
...
response = await fetchImpl(url, { method: 'POST', body: JSON.stringify(body), headers, ... });
...
data = encode(data);
const body = { data };
```

`encode` maps absent data to null (lines 41-44: `if (data == null) {
return null; }`), so the wire body is always `{"data": ...}`.
Optional context headers, when the providers are wired (lines
589-601): `Authorization: Bearer <idToken>`,
`Firebase-Instance-ID-Token`, `X-Firebase-AppCheck`. Default timeout
70 s -> `functions/deadline-exceeded` (lines 623-630, 421-438).

### The response (`callAtURL`, lines 645-657)

```js
let responseData = response.json.data;
// TODO(klimt): For right now, allow "result" instead of "data", for
// backwards compatibility.
if (typeof responseData === 'undefined') {
    responseData = response.json.result;
}
if (typeof responseData === 'undefined') {
    // Consider the response malformed.
    throw new FunctionsError('internal', 'Response is missing data field.', undefined, url);
}
```

A 2xx body that fails `response.json()` yields `json = null` and the
error `'internal', 'Response is not valid JSON object.'` (lines
571-581, 642-644).

### The error mapping (`_errorForResponse`, lines 213-290)

```js
function codeForHTTPStatus(status) {
    if (status >= 200 && status < 300) return 'ok';
    switch (status) {
        case 0: return 'internal';
        case 400: return 'invalid-argument';
        case 401: return 'unauthenticated';
        case 403: return 'permission-denied';
        case 404: return 'not-found';
        case 409: return 'aborted';
        case 429: return 'resource-exhausted';
        case 499: return 'cancelled';
        case 500: return 'internal';
        case 501: return 'unimplemented';
        case 503: return 'unavailable';
        case 504: return 'deadline-exceeded';
    }
    return 'unknown';
}
```

Body handling (lines 256-289): `errorJSON = bodyJSON.error`; its
`status` string must be a key of `errorCodeMap` (line 261, the map is
lines 154-172: `OK: 'ok'` ... `DATA_LOSS: 'data-loss'`) or the SDK
returns `new FunctionsError('internal', \`Unknown backend error status:
${statusText} [${httpStatus}]\`)`; otherwise the code is the mapped
one, `errorJSON.message` overrides the description, and
`errorJSON.details` is decoded and attached (`details !== undefined`,
lines 274-277). Success is `code === 'ok'` -> `null`. The thrown error
is `new FunctionsError(code, \`${description} [${httpStatus}]\`,
details, url)` and `FunctionsError` prefixes the code with
`functions/` (lines 180-204: `super(\`${FUNCTIONS_TYPE}/${code}\`, ...)`,
`FUNCTIONS_TYPE = 'functions'` at line 129).

## 2. The server side: firebase-functions v1

File: `src/common/providers/https.ts` on `firebase/firebase-functions`
(master, fetched via `gh api repos/firebase/firebase-functions/contents/src/common/providers/https.ts`).

Success (lines 978-986):

```ts
result = encode(result);
// If there was some result, encode it in the body.
const responseBody: HttpResponseBody = { result };
...
res.status(200).send(responseBody);
```

Error (lines 999-1010):

```ts
if (!(err instanceof HttpsError)) { ... httpErr = new HttpsError("internal", "INTERNAL"); }
const { status } = httpErr.httpErrorCode;
const body = { error: httpErr.toJSON() };
...
res.status(status).send(body);
```

`HttpsError.toJSON()` (lines 355-370) emits
`{ status: <canonical name>, message, details? }` where the canonical
name is UPPER_SNAKE (`errorCodeMap`, lines 290-306, e.g.
`"invalid-argument": { canonicalName: "INVALID_ARGUMENT", status: 400 }`).
The request body is decoded from `req.body.data` (line 924:
`const data: Req = decode(req.body.data);`).

So the two ends agree: client body `{"data": ...}` -> server body
`{"result": ...}` (client also accepts `{"data": ...}`), and server
error `{ "error": { "status": "INVALID_ARGUMENT", "message": ..., "details": ... } }`
with the mapped HTTP status -> client `functions/invalid-argument`.

## 3. The emulator's missing-function answer

File: `node_modules/firebase-tools/lib/emulator/functionsEmulator.js`
(the installed dev dependency), `handleHttpsTrigger` (lines
1127-1136):

```js
let triggerId = req.params.trigger_name;
if (req.params.region) {
    triggerId = `${req.params.region}-${triggerId}`;
}
if (!this.triggers[triggerId]) {
    res
        .status(404)
        .send(`Function ${triggerId} does not exist, valid functions are: ${Object.keys(this.triggers).join(", ")}`);
    return;
}
```

The body is plain text, not JSON, so the SDK's `response.json()` fails,
`json` stays null, and `_errorForResponse(404, null)` throws
`functions/not-found` with message `not-found [404]`.

## 4. The sandbox suite

- `emulator/firebase.json`: `"functions": { "port": 15001 }` (source
  `emulator/functions`).
- `emulator/.firebaserc`: `"default": "demo-nsh"`.
- `emulator/functions/index.js` (served by the suite;
  `firebase-functions/v1`):
  - `exports.ping = functions.https.onCall((data) => ({ pong: true, sent: data ?? null }));`
  - `exports.add = ...` returns `{ sum: a + b }` and throws
    `new functions.https.HttpsError("invalid-argument", "a and b must be numbers")`
    on NaN inputs.
- `scripts/test-emulator.sh` already lists
  `crates/nsh-firebase-functions/tests/emulator_functions.rs` and
  `packages/nsh-firebase/tests/emulator/functions.sandbox.test.tsx` in
  its run loop, and installs `emulator/functions/node_modules` first.

Consequently the sandbox URL for every call is
`http://127.0.0.1:15001/demo-nsh/us-central1/{name}`.
