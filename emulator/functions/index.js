// The demo project's callable functions, served by the Functions emulator
// on 127.0.0.1:15001 only. Two shapes: an echo and an add.
const functions = require("firebase-functions/v1");

exports.ping = functions.https.onCall((data) => ({ pong: true, sent: data ?? null }));

exports.add = functions.https.onCall((data) => {
  const a = Number(data && data.a);
  const b = Number(data && data.b);
  if (Number.isNaN(a) || Number.isNaN(b)) {
    throw new functions.https.HttpsError("invalid-argument", "a and b must be numbers");
  }
  return { sum: a + b };
});
