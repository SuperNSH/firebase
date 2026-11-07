//! Firebase Cloud Storage for Native Shell Rust apps.
//!
//! Resumable uploads on the Firebase `X-Goog-Upload-*` protocol (per
//! `docs/research.md`, verified against the firebase-js-sdk storage
//! layer): slices of 256 KiB, a session URI that survives process death,
//! and a progress signal with the caller's pause/resume/cancel. Reads
//! (download, download URL, metadata, list, delete) are plain REST on the
//! objects URL. The TypeScript package `packages/nsh-firebase/src/storage`
//! mirrors this behavior name for name.

pub mod storage;
pub mod upload;
pub mod wire;

pub use crate::{
    storage::{FirebaseStorage, StorageRef},
    upload::{UploadProgress, UploadState, UploadTask},
    wire::{
        CANCELED, CHUNK_SIZE, ListResult, NO_DEFAULT_BUCKET, NO_DOWNLOAD_TOKEN, OBJECT_NOT_FOUND,
        ObjectMetadata, QUOTA_EXCEEDED, RETRY_LIMIT_EXCEEDED, SERVER_FILE_WRONG_SIZE,
        SettableMetadata, UNAUTHENTICATED, UNAUTHORIZED, UNKNOWN,
    },
};
