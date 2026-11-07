//! Firebase Crashlytics for Native Shell Rust apps.
//!
//! The explicit-call facade over the tier facade of `nsh-firebase-core`:
//! calls ride a [`nsh_firebase_core::host::FirebaseHost`], and the pure
//! tier rejects every call with `crashlytics/unavailable` naming what is
//! missing. The TypeScript package
//! `packages/nsh-firebase/src/crashlytics` mirrors this behavior name
//! for name.
//!
//! There is no honest pure tier. Crashlytics has no REST, so a local
//! log-only recorder could never report a crash to Firebase — this
//! facade refuses to pretend otherwise: no queue, no local persistence,
//! no silent no-ops.
//!
//! # The contract
//!
//! The native areas (native/ios/NshFirebase, native/android) implement
//! this table. Both facades and both hosts are held to it verbatim:
//!
//! - `firebase.crashlytics.log({message}) -> {}`
//! - `firebase.crashlytics.setUserId({id}) -> {}`
//! - `firebase.crashlytics.setCustomKey({key, value}) -> {}`
//! - `firebase.crashlytics.recordError({code, message, stack?}) -> {}`
//! - `firebase.crashlytics.sendUnsentReports() -> {}`
//! - `firebase.crashlytics.didCrashDuringPreviousExecution() -> {crashed: bool}`
//!
//! The surface is deliberately small: the native SDK already captures
//! crashes (fatals, signals, ANRs) automatically; the wrapper adds only
//! the explicit calls — breadcrumbs, identity, custom keys, recorded
//! non-fatals, manual report flushing, and the previous-crash flag. RNFB
//! also exposes `crash()`, `checkForUnsentReports()`,
//! `deleteUnsentReports()` and `setCrashlyticsCollectionEnabled()`; they
//! have no handler here on purpose — that state machine belongs to the
//! native SDK, and a facade method that kills the process is test
//! tooling, not a library API.
//!
//! # Wire shapes
//!
//! `log` carries `{message}`; `setUserId` carries `{id}`;
//! `setCustomKey` carries `{key, value}` (string, number or boolean);
//! `recordError` carries `{code, message}` with `stack` added only when
//! the caller has one; `sendUnsentReports` and
//! `didCrashDuringPreviousExecution` carry no args. Replies: the five
//! act-only calls read no reply value, so either host's `{}`/`null`/
//! `true` resolves; `didCrashDuringPreviousExecution` reads `{crashed}`,
//! and a reply without the flag is `core/host-error`, never a silent
//! `false`.

pub mod crashlytics;
pub mod types;

pub use crate::{
    crashlytics::Crashlytics,
    types::{NEEDS_NATIVE_TIER, RecordError, UNAVAILABLE, wire},
};
