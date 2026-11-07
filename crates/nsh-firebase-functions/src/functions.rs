//! The Functions API of one app: `https_callable` over the pure tier's
//! fetch, or native routing through the `firebase.functions.call` handler
//! the embedding host registers. `packages/nsh-firebase/src/functions`
//! mirrors this name for name.

use std::sync::Arc;

use nsh_core::platform::HttpRequest;
use nsh_firebase_core::{EmulatorEndpoint, FirebaseApp, FirebaseError, FirebaseHost, error};
use serde_json::Value as JsonValue;

use crate::wire::{self, FunctionsConfig, FunctionsError, NATIVE_CALL, callable_url, request_body};

/// Read again before every request, so a refreshed token is honored.
pub type TokenProvider = dyn Fn() -> Option<String> + Send + Sync;

/// One named callable, as `httpsCallable(name)` hands it out.
pub struct HttpsCallable {
    functions: Functions,
    name: String,
}

impl HttpsCallable {
    /// The callable's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Calls the function with `data` (`Null` when absent) and resolves
    /// to the backend's result.
    pub async fn call(&self, data: JsonValue) -> Result<JsonValue, FunctionsError> {
        self.functions.call(&self.name, data).await
    }
}

/// The Cloud Functions API of one app.
#[derive(Clone)]
pub struct Functions {
    project_id: String,
    config: FunctionsConfig,
    host: Option<Arc<dyn FirebaseHost>>,
    auth: Option<Arc<TokenProvider>>,
}

impl Functions {
    /// The functions of `app`, on the sandbox default: the emulator at
    /// 127.0.0.1:15001, region us-central1.
    pub fn new(app: &Arc<FirebaseApp>) -> Self {
        Self::with_config(app, FunctionsConfig::default())
    }

    /// The functions of `app` with an explicit region/emulator switch.
    pub fn with_config(app: &Arc<FirebaseApp>, config: FunctionsConfig) -> Self {
        Self {
            project_id: app.options.project_id.clone(),
            config,
            host: None,
            auth: None,
        }
    }

    /// Routes every call through the host's `firebase.functions.call`
    /// handler instead of fetch, per the native contract.
    pub fn with_host(mut self, host: Arc<dyn FirebaseHost>) -> Self {
        self.host = Some(host);
        self
    }

    /// Supplies `Authorization: Bearer <idToken>` for callables that
    /// check auth (the SDK's context header).
    pub fn with_auth(mut self, auth: impl Fn() -> Option<String> + Send + Sync + 'static) -> Self {
        self.auth = Some(Arc::new(auth));
        self
    }

    /// Points calls at the Functions emulator (the SDK's
    /// `connectFunctionsEmulator`); a missing host is 127.0.0.1.
    pub fn connect_emulator(&mut self, host: Option<&str>, port: u16) {
        self.config.emulator = Some(EmulatorEndpoint {
            host: host.map(str::to_owned),
            port,
        });
    }

    /// The URL a call to `name` would use right now.
    pub fn callable_url(&self, name: &str) -> String {
        callable_url(&self.config, &self.project_id, name)
    }

    /// `httpsCallable(name)`: the callable reference.
    pub fn https_callable(&self, name: impl Into<String>) -> HttpsCallable {
        HttpsCallable {
            functions: self.clone(),
            name: name.into(),
        }
    }

    /// Calls `name` with `data`: through the host handler when one is
    /// wired ("when the host answers"), otherwise a plain POST.
    pub async fn call(&self, name: &str, data: JsonValue) -> Result<JsonValue, FunctionsError> {
        match &self.host {
            Some(host) => call_through_host(host, name, data).await,
            None => self.call_over_fetch(name, data).await,
        }
    }

    async fn call_over_fetch(
        &self,
        name: &str,
        data: JsonValue,
    ) -> Result<JsonValue, FunctionsError> {
        // The POST must run on the engine thread: that is where fetch
        // lives, and capabilities.network gates the host.
        let mut request =
            HttpRequest::post(self.callable_url(name), request_body(&data).into_bytes())
                .header("Content-Type", "application/json");
        if let Some(token) = self.auth.as_deref().and_then(|auth| auth()) {
            request = request.header("Authorization", format!("Bearer {token}"));
        }
        let response = nsh_firebase_core::send(request)
            .await
            .map_err(FunctionsError::from)?;
        wire::result_from_response(response.status(), &response.text())
    }
}

/// The native contract (coordinator-fixed): the handler takes
/// `{name, data}` and answers `{result}`; a refusal or a handler without
/// a result surfaces as its mapped error.
async fn call_through_host(
    host: &Arc<dyn FirebaseHost>,
    name: &str,
    data: JsonValue,
) -> Result<JsonValue, FunctionsError> {
    let payload = host
        .call(
            NATIVE_CALL,
            serde_json::json!({ "name": name, "data": data }),
        )
        .await
        .map_err(FunctionsError::from)?;
    match payload.get("result") {
        Some(result) => Ok(result.clone()),
        None => Err(FunctionsError::from(FirebaseError::new(
            error::HOST_ERROR,
            "firebase.functions.call returned no `result`",
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Mutex;

    fn app() -> Arc<FirebaseApp> {
        nsh_firebase_core::initialize_app(
            nsh_firebase_core::FirebaseOptions::demo(),
            "functions-service-test",
        )
        .unwrap()
    }

    /// Records calls and answers them from a canned table.
    #[derive(Default)]
    struct RecordingHost {
        calls: Mutex<Vec<(String, JsonValue)>>,
        answers: Mutex<Vec<JsonValue>>,
        refused: bool,
    }

    impl RecordingHost {
        fn calls(&self) -> Vec<(String, JsonValue)> {
            self.calls.lock().expect("recording host lock").clone()
        }
    }

    impl FirebaseHost for RecordingHost {
        fn call(&self, name: &str, args: JsonValue) -> nsh_firebase_core::host::HostCallFuture {
            self.calls
                .lock()
                .expect("recording host lock")
                .push((name.to_owned(), args));
            if self.refused {
                let name = name.to_owned();
                return Box::pin(async move {
                    Err(FirebaseError::from_host_code(
                        "ERR_ACCESS_DENIED",
                        format!("`{name}` is not granted"),
                    ))
                });
            }
            let answer = self
                .answers
                .lock()
                .expect("recording host lock")
                .pop()
                .unwrap_or(JsonValue::Null);
            Box::pin(async move { Ok(answer) })
        }

        fn post(&self, _name: &str, _args: JsonValue) -> Result<(), FirebaseError> {
            unimplemented!("functions never posts");
        }
    }

    #[tokio::test]
    async fn native_routing_sends_the_name_and_data_and_unwraps_result() {
        let host = Arc::new(RecordingHost {
            answers: Mutex::new(vec![json!({ "result": { "sum": 5 } })]),
            ..RecordingHost::default()
        });
        let functions =
            Functions::new(&app()).with_host(Arc::clone(&host) as Arc<dyn FirebaseHost>);
        let result = functions
            .call("add", json!({ "a": 2, "b": 3 }))
            .await
            .unwrap();
        assert_eq!(result, json!({ "sum": 5 }));
        assert_eq!(
            host.calls(),
            vec![(
                NATIVE_CALL.to_owned(),
                json!({ "name": "add", "data": { "a": 2, "b": 3 } }),
            )]
        );
    }

    #[tokio::test]
    async fn native_routing_sends_null_data_when_absent() {
        let host = Arc::new(RecordingHost {
            answers: Mutex::new(vec![json!({ "result": { "pong": true } })]),
            ..RecordingHost::default()
        });
        let functions =
            Functions::new(&app()).with_host(Arc::clone(&host) as Arc<dyn FirebaseHost>);
        functions.call("ping", JsonValue::Null).await.unwrap();
        assert_eq!(
            host.calls(),
            vec![(
                NATIVE_CALL.to_owned(),
                json!({ "name": "ping", "data": null })
            )]
        );
    }

    #[tokio::test]
    async fn a_handler_without_a_result_is_a_host_error() {
        let host = Arc::new(RecordingHost {
            answers: Mutex::new(vec![json!({ "error": { "status": "INVALID_ARGUMENT" } })]),
            ..RecordingHost::default()
        });
        let functions =
            Functions::new(&app()).with_host(Arc::clone(&host) as Arc<dyn FirebaseHost>);
        let failure = functions.call("add", JsonValue::Null).await.unwrap_err();
        assert_eq!(failure.code, error::HOST_ERROR);
    }

    #[tokio::test]
    async fn a_refused_handler_keeps_its_native_code() {
        let host = Arc::new(RecordingHost {
            refused: true,
            ..RecordingHost::default()
        });
        let functions =
            Functions::new(&app()).with_host(Arc::clone(&host) as Arc<dyn FirebaseHost>);
        let failure = functions.call("ping", JsonValue::Null).await.unwrap_err();
        assert_eq!(failure.code, error::ACCESS_DENIED);
        assert_eq!(failure.native_code.as_deref(), Some("ERR_ACCESS_DENIED"));
    }

    #[test]
    fn the_default_config_is_the_sandbox_emulator() {
        let functions = Functions::new(&app());
        assert_eq!(
            functions.callable_url("ping"),
            "http://127.0.0.1:15001/demo-nsh/us-central1/ping"
        );
    }

    #[test]
    fn connect_emulator_repoints_calls() {
        let mut functions = Functions::new(&app());
        functions.connect_emulator(Some("10.0.0.9"), 5001);
        assert_eq!(
            functions.callable_url("add"),
            "http://10.0.0.9:5001/demo-nsh/us-central1/add"
        );
        functions.connect_emulator(None, 15001);
        assert_eq!(
            functions.callable_url("add"),
            "http://127.0.0.1:15001/demo-nsh/us-central1/add"
        );
    }

    #[test]
    fn https_callable_keeps_its_name() {
        let functions = Functions::new(&app());
        assert_eq!(functions.https_callable("ping").name(), "ping");
        assert_eq!(functions.https_callable("add").name(), "add");
    }
}
