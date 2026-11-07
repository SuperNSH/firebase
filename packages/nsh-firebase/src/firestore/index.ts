// Firebase Cloud Firestore for Native Shell TSX apps: documents CRUD and
// shallow structured queries over the v1 REST surface, or the native
// firebase.firestore.* handlers when the host answers. The Rust crate
// `nsh-firebase-firestore` mirrors this behavior name for name.
//
// The pure tier talks to the suite's Firestore emulator
// (127.0.0.1:18080) by default; `firestore(app, { restBase })` points it
// elsewhere. There are no realtime listeners here: the REST surface has
// none to speak of, and none is faked.

export {
  ADD_DOC,
  ALREADY_EXISTS,
  ABORTED,
  CANCELLED,
  DATA_LOSS,
  DEADLINE_EXCEEDED,
  DEFAULT_DATABASE,
  DELETE_DOC,
  EMULATOR_BASE,
  EMULATOR_PORT,
  GET_DOC,
  INTERNAL,
  INVALID_ARGUMENT_CODE,
  INVALID_DATA,
  INVALID_RESPONSE,
  NOT_FOUND,
  OUT_OF_RANGE,
  PERMISSION_DENIED,
  PRODUCTION_BASE,
  RESOURCE_EXHAUSTED,
  RUN_QUERY,
  SET_DOC,
  UNAUTHENTICATED,
  UNAVAILABLE,
  UNIMPLEMENTED,
  UNKNOWN,
  FAILED_PRECONDITION,
  collectionUrl,
  decodeFields,
  decodeValue,
  documentsBase,
  documentUrl,
  docsFromRunQuery,
  encodeFields,
  encodeSegment,
  encodeValue,
  ensureOk,
  errorForResponse,
  idFromName,
  invalidArgument,
  invalidData,
  invalidResponse,
  leafPaths,
  mergeMaskQuery,
  parseBody,
  queryDocFrom,
  runQueryPlan,
  snapshotFrom,
  splitCollectionPath,
  splitDocPath,
} from "./wire";
export type {
  Direction,
  DocSnapshot,
  FirestoreQuery,
  Json,
  OrderBy,
  QueryDoc,
  QueryPlan,
  WhereFilter,
  WhereOp,
} from "./wire";
export { Firestore, firestore } from "./firestore";
export type { FirestoreSettings } from "./firestore";
