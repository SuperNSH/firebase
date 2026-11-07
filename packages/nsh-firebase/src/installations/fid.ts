// FID generation, mirroring the js-SDK's generateFid (firebase-js-sdk
// packages/installations/src/helpers/generate-fid.ts) and
// crates/nsh-firebase-installations/src/fid.rs: 17 random bytes, the
// first byte's top 4 bits replaced with the FID header 0b0111,
// base64url, cut to 22 characters — a valid FID starts with c-f and
// holds only [\w-] after it.

// The table bufferToBase64UrlSafe encodes with (RFC 4648 §5, -/_ for
// +/).
const TABLE = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

// The pattern the js-SDK validates with (/^[cdef][\w-]{21}$/), spelled
// out.
export const VALID_FID_PATTERN = /^[cdef][A-Za-z0-9_-]{21}$/;

// One FID; crypto.getRandomValues is the app's randomness (nsh globals).
export function generateFid(): string {
  const bytes = new Uint8Array(17);
  crypto.getRandomValues(bytes);
  // Replace the first 4 random bits with the constant FID header.
  bytes[0] = 0b01110000 + (bytes[0] % 0b00010000);
  return base64Url(bytes).slice(0, 22);
}

// Whether fid is a shape the service takes: 22 base64url characters with
// the FID header. A record that fails it is corrupt, not absent.
export function isValidFid(fid: string): boolean {
  return VALID_FID_PATTERN.test(fid);
}

// RFC 4648 §5 without padding: 17 bytes land at 23 characters (the last
// carries 4 bits), and the caller cuts at 22 like the js-SDK's
// substr(0, 22) does.
function base64Url(bytes: Uint8Array): string {
  let out = "";
  for (let at = 0; at < bytes.length; at += 3) {
    const b = [bytes[at], bytes[at + 1] ?? 0, bytes[at + 2] ?? 0];
    const n = (b[0] << 16) | (b[1] << 8) | b[2];
    out += TABLE[(n >> 18) & 63];
    out += TABLE[(n >> 12) & 63];
    if (at + 1 < bytes.length) out += TABLE[(n >> 6) & 63];
    if (at + 2 < bytes.length) out += TABLE[n & 63];
  }
  return out;
}
