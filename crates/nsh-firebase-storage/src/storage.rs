//! Storage references and their reads: download, download URL, metadata,
//! list and delete over the pure tier's fetch. The resumable upload lives
//! in [`crate::upload`]. `packages/nsh-firebase/src/storage/storage.ts`
//! mirrors this name for name.

use std::{cell::Cell, rc::Rc, sync::Arc};

use nsh_core::{element::HttpResponse, platform::HttpRequest, prelude::*};
use nsh_firebase_core::{FirebaseApp, FirebaseError};
use serde_json::Value as JsonValue;

use crate::{
    upload::{
        UploadJob, UploadProgress, UploadState, UploadTask, UploadTaskInner, map_status_error,
        run_loop,
    },
    wire::{self, ListResult, ObjectMetadata, SettableMetadata},
};

/// The Cloud Storage API of one app.
#[derive(Clone)]
pub struct FirebaseStorage {
    /// `{origin}/v0/b/<bucket>/o`: uploads start here, lists and object
    /// reads hang off it.
    objects_base: String,
    /// The origin alone, which download URLs are built from.
    origin: String,
    auth: Option<Arc<dyn Fn() -> Option<String> + Send + Sync>>,
}

impl FirebaseStorage {
    /// The storage of `app`: its emulator switch (or the production REST
    /// host) plus its bucket.
    pub fn new(app: &Arc<FirebaseApp>) -> Result<Self, FirebaseError> {
        let origin = wire::origin(&app.options)?;
        let bucket = app.options.storage_bucket.clone().ok_or_else(|| {
            FirebaseError::new(
                wire::NO_DEFAULT_BUCKET,
                "FirebaseOptions.storageBucket is empty",
            )
        })?;
        Ok(Self {
            objects_base: wire::objects_base(&origin, &bucket),
            origin,
            auth: None,
        })
    }

    /// Supplies `Authorization: Firebase <idToken>` for private objects;
    /// read again on every request, so a refreshed token is honored.
    pub fn with_auth(mut self, auth: impl Fn() -> Option<String> + Send + Sync + 'static) -> Self {
        self.auth = Some(Arc::new(auth));
        self
    }

    /// A reference of `path` under this bucket (`""` is the root, for
    /// list); a leading slash is dropped, as the SDK tolerates.
    pub fn r#ref(&self, path: &str) -> StorageRef {
        StorageRef {
            storage: self.clone(),
            path: path.trim_matches('/').to_owned(),
        }
    }

    /// The auth header of this moment, if a provider is wired.
    pub(crate) fn auth_header(&self) -> Vec<(&'static str, String)> {
        let provider: Option<&(dyn Fn() -> Option<String> + 'static)> =
            self.auth.as_deref().map(|auth| auth as _);
        crate::upload::auth_header(provider)
    }
}

/// One object path in the bucket.
#[derive(Clone)]
pub struct StorageRef {
    storage: FirebaseStorage,
    path: String,
}

impl StorageRef {
    /// The path, without a leading or trailing slash.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Starts a resumable upload of `bytes` (`metadata` is optional):
    /// returns the task at once, running its loop as the caller's task.
    pub fn upload(&self, bytes: Vec<u8>, metadata: Option<SettableMetadata>) -> UploadTask {
        self.start(bytes, None, metadata)
    }

    /// Continues a previous session by its saved id (the session URI the
    /// first run got): queries the server's offset, uploads only the rest.
    pub fn resume_upload(&self, bytes: Vec<u8>, session_url: impl Into<String>) -> UploadTask {
        self.start(bytes, Some(session_url.into()), None)
    }

    fn start(
        &self,
        bytes: Vec<u8>,
        session_url: Option<String>,
        metadata: Option<SettableMetadata>,
    ) -> UploadTask {
        let total = bytes.len() as u64;
        // The start body: the metadata's writable fields plus the name
        // (metadataForUpload_); the content type defaults to the header.
        let mut resource = metadata
            .as_ref()
            .map(SettableMetadata::resource)
            .unwrap_or_default();
        let content_type = metadata
            .as_ref()
            .and_then(|metadata| metadata.content_type.clone())
            .unwrap_or_else(|| "application/octet-stream".to_owned());
        resource["name"] = JsonValue::String(self.path.clone());
        resource["contentType"] = JsonValue::String(content_type.clone());
        let inner = Rc::new(UploadTaskInner {
            progress: signal(UploadProgress {
                bytes_transferred: 0,
                total_bytes: total,
                state: UploadState::Running,
            }),
            result: signal(None),
            paused: Cell::new(false),
            cancelled: Cell::new(false),
            job: UploadJob {
                start_url: wire::start_url(&self.storage.objects_base, &self.path),
                path: self.path.clone(),
                start_body: resource.to_string().into_bytes(),
                start_content_type: content_type,
                session_url: std::cell::RefCell::new(session_url),
                bytes: Rc::new(bytes),
            },
            auth: self.storage.auth.as_ref().map(|auth| {
                let auth = Arc::clone(auth);
                Rc::new(move || auth()) as Rc<dyn Fn() -> Option<String>>
            }),
        });
        spawn(run_loop(Rc::new(UploadTask::new(Rc::clone(&inner)))));
        UploadTask::new(inner)
    }

    /// The object's bytes (`alt=media`).
    pub async fn download(&self) -> Result<Vec<u8>, FirebaseError> {
        let response = self
            .request(HttpRequest::get(format!(
                "{}?alt=media",
                wire::object_url(&self.storage.objects_base, &self.path)
            )))
            .await?;
        let response = self.ensure_ok(response)?;
        Ok(response.body)
    }

    /// The first token's download URL from the object's metadata.
    pub async fn download_url(&self) -> Result<String, FirebaseError> {
        let metadata = self.get_metadata().await?;
        metadata.download_url(&self.storage.origin).ok_or_else(|| {
            FirebaseError::new(
                wire::NO_DOWNLOAD_TOKEN,
                format!("{} has no download token", self.path),
            )
        })
    }

    /// The object's metadata as the server holds it.
    pub async fn get_metadata(&self) -> Result<ObjectMetadata, FirebaseError> {
        let response = self
            .request(HttpRequest::get(wire::object_url(
                &self.storage.objects_base,
                &self.path,
            )))
            .await?;
        let response = self.ensure_ok(response)?;
        Ok(ObjectMetadata::parse(&Self::parse_json(
            &response, "metadata",
        )?))
    }

    /// Patches the settable metadata and returns what the server holds.
    pub async fn update_metadata(
        &self,
        change: SettableMetadata,
    ) -> Result<ObjectMetadata, FirebaseError> {
        let response = self
            .request(
                HttpRequest::new(
                    "PATCH",
                    wire::object_url(&self.storage.objects_base, &self.path),
                )
                .body(change.resource().to_string().into_bytes())
                .header("Content-Type", "application/json; charset=utf-8"),
            )
            .await?;
        let response = self.ensure_ok(response)?;
        Ok(ObjectMetadata::parse(&Self::parse_json(
            &response, "metadata",
        )?))
    }

    /// One page of the objects under this prefix: full paths, the
    /// `prefixes` the delimiter folded, and the next page's token.
    pub async fn list(
        &self,
        max_results: Option<u32>,
        page_token: Option<&str>,
    ) -> Result<ListResult, FirebaseError> {
        let prefix = if self.path.is_empty() {
            String::new()
        } else {
            format!("{}/", self.path)
        };
        let mut url = format!(
            "{}?prefix={}&delimiter=/",
            self.storage.objects_base,
            wire::encode_component(&prefix)
        );
        if let Some(token) = page_token {
            url.push_str(&format!("&pageToken={}", wire::encode_component(token)));
        }
        if let Some(max_results) = max_results {
            url.push_str(&format!("&maxResults={max_results}"));
        }
        let response = self.request(HttpRequest::get(url)).await?;
        let response = self.ensure_ok(response)?;
        Ok(ListResult::parse(&Self::parse_json(
            &response,
            "the list page",
        )?))
    }

    /// Deletes the object; deleting a missing one is `object-not-found`.
    pub async fn delete(&self) -> Result<(), FirebaseError> {
        let response = self
            .request(HttpRequest::new(
                "DELETE",
                wire::object_url(&self.storage.objects_base, &self.path),
            ))
            .await?;
        self.ensure_ok(response)?;
        Ok(())
    }

    /// Sends `request` with the auth header wired on this storage.
    async fn request(&self, mut request: HttpRequest) -> Result<HttpResponse, FirebaseError> {
        for (name, value) in self.storage.auth_header() {
            request = request.header(name, value);
        }
        nsh_firebase_core::send(request).await
    }

    /// An HTTP failure becomes a storage error naming the object.
    fn ensure_ok(&self, response: HttpResponse) -> Result<HttpResponse, FirebaseError> {
        if response.ok() {
            return Ok(response);
        }
        Err(map_status_error(
            response.status(),
            &self.path,
            &response.text(),
        ))
    }

    fn parse_json(response: &HttpResponse, what: &str) -> Result<JsonValue, FirebaseError> {
        serde_json::from_str(&response.text()).map_err(|error| {
            FirebaseError::new(wire::UNKNOWN, format!("{what} is not JSON: {error}"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refs_resolve_against_the_emulator_objects_url() {
        let app = nsh_firebase_core::initialize_app(
            nsh_firebase_core::FirebaseOptions::demo(),
            "storage-unit",
        )
        .unwrap();
        let storage = FirebaseStorage::new(&app).unwrap();
        assert_eq!(storage.r#ref("demo/upload.txt").path(), "demo/upload.txt");
        // A leading slash is tolerated and dropped.
        assert_eq!(storage.r#ref("/demo/upload.txt").path(), "demo/upload.txt");
        assert_eq!(storage.r#ref("").path(), "");
        assert_eq!(storage.r#ref("a/b/").path(), "a/b");
    }
}
