// The remote-config stable codes, shared with
// crates/nsh-firebase-remote-config/src/codes.rs. Argument validation
// keeps the package-wide core/invalid-argument.
export const RC_CODES = {
  unavailable: "remote-config/unavailable",
  invalidTemplate: "remote-config/invalid-template",
} as const;
