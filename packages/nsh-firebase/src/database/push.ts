// Client-generated push ids with the Firebase ordering properties, the
// TypeScript twin of crates/nsh-firebase-database/src/push.rs. The
// algorithm is the official client's `nextPushId`
// (`@firebase/database`, `packages/database/src/core/util/NextPushId.ts`,
// the keys the REST `POST` of
// https://firebase.google.com/docs/database/rest/save-data describes):
// 8 characters of the millisecond timestamp in the 64-character
// web-safe-ordered alphabet, then 12 characters (72 bits) of per-client
// randomness. Ids sort lexicographically in time order and keep
// increasing when two are made within the same millisecond.

const PUSH_CHARS = "-0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ_abcdefghijklmnopqrstuvwxyz";
const RAND_CHARS = 12;

let lastMs = 0;
let lastRand: number[] = [];

// One push id for `nowMs` (the app clock's millisecond: a manual test
// clock makes same-millisecond pushes repeatable).
export function pushId(nowMs: number = Date.now()): string {
  const duplicate = nowMs === lastMs;
  lastMs = nowMs;
  let now = nowMs;
  const chars: string[] = [];
  for (let i = 0; i < 8; i++) {
    chars.unshift(PUSH_CHARS[now % 64]);
    now = Math.floor(now / 64);
  }
  if (now !== 0) throw new Error("cannot push at a time past the 8 timestamp characters");
  if (duplicate) {
    // Same millisecond: keep the randomness, incremented by one, so
    // this id sorts right after the last one.
    let i = RAND_CHARS - 1;
    for (; i > 0 && lastRand[i] === 63; i--) lastRand[i] = 0;
    lastRand[i] = (lastRand[i] + 1) % 64;
  } else {
    lastRand = Array.from({ length: RAND_CHARS }, () => Math.floor(Math.random() * 64));
  }
  return chars.join("") + lastRand.map((index) => PUSH_CHARS[index]).join("");
}
