//! App options and the emulator switches.

use serde_json::Value as JsonValue;

use crate::error::FirebaseError;

/// One emulator endpoint: host with a default, port required.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmulatorEndpoint {
    /// Missing: `127.0.0.1`, the sandbox's.
    pub host: Option<String>,
    pub port: u16,
}

impl EmulatorEndpoint {
    pub fn new(port: u16) -> Self {
        Self { host: None, port }
    }

    /// The base URL of the endpoint, `http://host:port`.
    pub fn base_url(&self) -> String {
        format!(
            "http://{}:{}",
            self.host.as_deref().unwrap_or("127.0.0.1"),
            self.port
        )
    }
}

/// Per-product emulator switches; a product with a switch ignores the
/// real URL fields and talks to the emulator instead.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EmulatorOptions {
    pub auth: Option<EmulatorEndpoint>,
    pub database: Option<EmulatorEndpoint>,
    pub storage: Option<EmulatorEndpoint>,
}

/// The options `initializeApp` takes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FirebaseOptions {
    pub api_key: String,
    pub app_id: String,
    pub project_id: String,
    pub database_url: Option<String>,
    pub auth_url: Option<String>,
    /// The emulator's bucket name, without a `gs://` prefix.
    pub storage_bucket: Option<String>,
    pub emulator: EmulatorOptions,
}

impl FirebaseOptions {
    /// The sandbox defaults: `demo-nsh` needs no account and no
    /// credentials; every value here is fake.
    pub fn demo() -> Self {
        Self {
            api_key: "demo-nsh-fake-api-key".to_owned(),
            app_id: "1:000000000000:web:demo-nsh".to_owned(),
            project_id: "demo-nsh".to_owned(),
            database_url: Some("http://127.0.0.1:19000".to_owned()),
            auth_url: Some("http://127.0.0.1:19099".to_owned()),
            storage_bucket: Some("demo-nsh.firebasestorage.app".to_owned()),
            emulator: EmulatorOptions {
                auth: Some(EmulatorEndpoint::new(19_099)),
                database: Some(EmulatorEndpoint::new(19_000)),
                storage: Some(EmulatorEndpoint::new(19_199)),
            },
        }
    }

    /// A project id is the one option no call can do without.
    pub fn validate(&self) -> Result<(), FirebaseError> {
        if self.project_id.is_empty() {
            Err(FirebaseError::new(
                crate::error::INVALID_ARGUMENT,
                "FirebaseOptions.project_id is empty",
            ))
        } else {
            Ok(())
        }
    }

    /// The base URL auth talks to: the emulator switch wins over
    /// `auth_url`.
    pub fn auth_base_url(&self) -> Option<String> {
        match &self.emulator.auth {
            Some(endpoint) => Some(endpoint.base_url()),
            None => self.auth_url.clone(),
        }
    }

    /// The base URL of Realtime Database: the emulator switch wins over
    /// `database_url`.
    pub fn database_base_url(&self) -> Option<String> {
        match &self.emulator.database {
            Some(endpoint) => Some(endpoint.base_url()),
            None => self.database_url.clone(),
        }
    }

    /// The base URL Cloud Storage talks to: the emulator switch wins over
    /// the real REST host (unprefixed; the bucket goes in the path).
    pub fn storage_base_url(&self) -> Option<String> {
        match &self.emulator.storage {
            Some(endpoint) => Some(endpoint.base_url()),
            None => self
                .storage_bucket
                .as_ref()
                .map(|bucket| format!("https://firebasestorage.googleapis.com/v0/b/{bucket}/o")),
        }
    }
}

/// Reads the options from a config document: `apiKey`, `appId`,
/// `projectId`, `databaseUrl`, `authUrl`, `storageBucket` and an optional
/// `emulator` block with per-product `{host, port}` endpoints. A wrong
/// shape or an empty projectId is INVALID_ARGUMENT.
pub fn from_config(config: &JsonValue) -> Result<FirebaseOptions, FirebaseError> {
    let object = config
        .as_object()
        .ok_or_else(|| shape_error("the config is not a JSON object"))?;
    let string = |key: &str| -> Result<Option<String>, FirebaseError> {
        match object.get(key) {
            None | Some(JsonValue::Null) => Ok(None),
            Some(value) => value
                .as_str()
                .map(str::to_owned)
                .map(Some)
                .ok_or_else(|| shape_error(format!("{key} is not a string"))),
        }
    };
    let emulator = match object.get("emulator") {
        None | Some(JsonValue::Null) => None,
        Some(value) => Some(
            value
                .as_object()
                .ok_or_else(|| shape_error("emulator is not an object"))?,
        ),
    };
    let endpoint = |product: &str| -> Result<Option<EmulatorEndpoint>, FirebaseError> {
        let value = emulator
            .and_then(|block| block.get(product))
            .filter(|value| !value.is_null());
        match value {
            None => Ok(None),
            Some(value) => {
                if !value.is_object() {
                    return Err(shape_error(format!("emulator.{product} is not an object")));
                }
                // An explicit null host counts as absent, like every
                // string field; anything else non-string is a wrong shape.
                let host: Option<String> = match value.get("host") {
                    None | Some(JsonValue::Null) => None,
                    Some(host) => Some(
                        host.as_str()
                            .ok_or_else(|| {
                                shape_error(format!("emulator.{product}.host is not a string"))
                            })?
                            .to_owned(),
                    ),
                };
                // A JSON document may write the port as 19000.0, which
                // as_u64 rejects and TypeScript's JSON.parse accepts as
                // the integer; take an integral, non-negative float too.
                let raw_port = value.get("port");
                let port = raw_port
                    .and_then(|port| port.as_u64())
                    .or_else(|| {
                        raw_port
                            .and_then(|port| port.as_f64())
                            .filter(|float| float.fract() == 0.0 && *float >= 0.0)
                            .map(|float| float as u64)
                    })
                    .ok_or_else(|| {
                        shape_error(format!("emulator.{product}.port is not a number"))
                    })?;
                Ok(Some(EmulatorEndpoint {
                    host,
                    port: u16::try_from(port).map_err(|_| {
                        shape_error(format!("emulator.{product}.port is out of range"))
                    })?,
                }))
            }
        }
    };
    let options = FirebaseOptions {
        api_key: string("apiKey")?.unwrap_or_default(),
        app_id: string("appId")?.unwrap_or_default(),
        project_id: string("projectId")?.unwrap_or_default(),
        database_url: string("databaseUrl")?,
        auth_url: string("authUrl")?,
        storage_bucket: string("storageBucket")?,
        emulator: EmulatorOptions {
            auth: endpoint("auth")?,
            database: endpoint("database")?,
            storage: endpoint("storage")?,
        },
    };
    options.validate()?;
    Ok(options)
}

fn shape_error(message: impl Into<String>) -> FirebaseError {
    FirebaseError::new(crate::error::INVALID_ARGUMENT, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The fallback URLs are deliberately distinct from the emulator
    // ports, so a regression that ignores them cannot pass.
    const CONFIG: &str = r#"{
        "apiKey": "demo-nsh-fake-api-key",
        "appId": "1:000000000000:web:demo-nsh",
        "projectId": "demo-nsh",
        "databaseUrl": "http://10.9.9.9:17000",
        "authUrl": "http://10.9.9.8:17099",
        "storageBucket": "demo-nsh.firebasestorage.app",
        "emulator": {
            "auth": { "host": "127.0.0.1", "port": 19099 },
            "database": { "host": "127.0.0.1", "port": 19000 },
            "storage": { "port": 19199 }
        }
    }"#;

    #[test]
    fn from_config_reads_the_demo_fixture() {
        let config: JsonValue = serde_json::from_str(CONFIG).unwrap();
        let options = from_config(&config).unwrap();
        assert_eq!(options.project_id, "demo-nsh");
        assert_eq!(options.api_key, "demo-nsh-fake-api-key");
        assert_eq!(
            options.database_base_url(),
            Some("http://127.0.0.1:19000".to_owned())
        );
        assert_eq!(
            options.auth_base_url(),
            Some("http://127.0.0.1:19099".to_owned())
        );
        assert_eq!(
            options.storage_base_url(),
            Some("http://127.0.0.1:19199".to_owned())
        );
    }

    #[test]
    fn from_config_falls_back_to_the_document_urls_without_an_emulator_block() {
        let mut config: JsonValue = serde_json::from_str(CONFIG).unwrap();
        config["emulator"] = JsonValue::Null;
        let options = from_config(&config).unwrap();
        assert_eq!(
            options.database_base_url(),
            Some("http://10.9.9.9:17000".to_owned())
        );
        assert_eq!(
            options.auth_base_url(),
            Some("http://10.9.9.8:17099".to_owned())
        );
        // The REST host is unprefixed; the bucket goes in the path.
        assert_eq!(
            options.storage_base_url(),
            Some(
                "https://firebasestorage.googleapis.com/v0/b/demo-nsh.firebasestorage.app/o"
                    .to_owned()
            )
        );
    }

    #[test]
    fn from_config_rejects_wrong_shapes() {
        let not_an_object = from_config(&JsonValue::Null).unwrap_err();
        assert_eq!(not_an_object.code, crate::error::INVALID_ARGUMENT);

        let bad_api_key: JsonValue = serde_json::json!({ "projectId": "p", "apiKey": 7 });
        let error = from_config(&bad_api_key).unwrap_err();
        assert_eq!(error.code, crate::error::INVALID_ARGUMENT);

        // The messages must match the TypeScript optionsFromConfig.
        let string_endpoint: JsonValue =
            serde_json::json!({ "projectId": "p", "emulator": { "database": "x" } });
        let error = from_config(&string_endpoint).unwrap_err();
        assert_eq!(error.code, crate::error::INVALID_ARGUMENT);
        assert_eq!(error.message, "emulator.database is not an object");

        let bad_port: JsonValue =
            serde_json::json!({ "projectId": "p", "emulator": { "database": { "port": "x" } } });
        let error = from_config(&bad_port).unwrap_err();
        assert_eq!(error.code, crate::error::INVALID_ARGUMENT);
        assert_eq!(error.message, "emulator.database.port is not a number");

        let out_of_range: JsonValue =
            serde_json::json!({ "projectId": "p", "emulator": { "database": { "port": 70000 } } });
        let error = from_config(&out_of_range).unwrap_err();
        assert_eq!(error.code, crate::error::INVALID_ARGUMENT);
        assert_eq!(error.message, "emulator.database.port is out of range");

        let bad_host: JsonValue = serde_json::json!(
            { "projectId": "p", "emulator": { "database": { "host": 19000, "port": 19000 } } }
        );
        let error = from_config(&bad_host).unwrap_err();
        assert_eq!(error.code, crate::error::INVALID_ARGUMENT);
        assert_eq!(error.message, "emulator.database.host is not a string");

        // A float out of the u16 range keeps the range message, the same
        // one TypeScript throws for the integer JSON.parse produces.
        let float_out_of_range: JsonValue = serde_json::json!({ "projectId": "p", "emulator": { "database": { "port": 70000.0 } } });
        let error = from_config(&float_out_of_range).unwrap_err();
        assert_eq!(error.code, crate::error::INVALID_ARGUMENT);
        assert_eq!(error.message, "emulator.database.port is out of range");

        let no_project = from_config(&serde_json::json!({})).unwrap_err();
        assert_eq!(no_project.code, crate::error::INVALID_ARGUMENT);
    }

    #[test]
    fn a_json_float_port_is_taken_as_its_integer() {
        // {"port":19000.0} parses as a Float, which as_u64 rejects; the
        // loader must accept it like TypeScript's JSON.parse does.
        let config: JsonValue = serde_json::json!(
            { "projectId": "p", "emulator": { "database": { "host": "127.0.0.1", "port": 19000.0 } } }
        );
        let options = from_config(&config).unwrap();
        assert_eq!(
            options.database_base_url(),
            Some("http://127.0.0.1:19000".to_owned())
        );
    }

    #[test]
    fn an_explicit_null_host_is_absent() {
        let config: JsonValue = serde_json::json!(
            { "projectId": "p", "emulator": { "database": { "host": null, "port": 19000 } } }
        );
        let options = from_config(&config).unwrap();
        assert_eq!(
            options.database_base_url(),
            Some("http://127.0.0.1:19000".to_owned())
        );
    }

    #[test]
    fn demo_defaults_are_the_sandbox_suite() {
        let options = FirebaseOptions::demo();
        assert_eq!(options.project_id, "demo-nsh");
        assert_eq!(
            options.database_base_url(),
            Some("http://127.0.0.1:19000".to_owned())
        );
        assert_eq!(
            options.auth_base_url(),
            Some("http://127.0.0.1:19099".to_owned())
        );
        assert_eq!(
            options.storage_base_url(),
            Some("http://127.0.0.1:19199".to_owned())
        );
    }

    #[test]
    fn a_host_override_changes_the_endpoint_but_keeps_the_scheme() {
        let mut options = FirebaseOptions::demo();
        options.emulator.database = Some(EmulatorEndpoint {
            host: Some("10.0.0.2".to_owned()),
            port: 9001,
        });
        assert_eq!(
            options.database_base_url(),
            Some("http://10.0.0.2:9001".to_owned())
        );
    }

    #[test]
    fn without_emulator_switches_the_real_urls_stand() {
        let mut options = FirebaseOptions::demo();
        // Distinct from the emulator ports on purpose: a fallback that
        // ignores database_url and hardcodes 19000 fails here.
        options.database_url = Some("http://10.9.9.9".to_owned());
        options.auth_url = Some("http://10.9.9.7".to_owned());
        options.emulator = EmulatorOptions::default();
        assert_eq!(
            options.database_base_url(),
            Some("http://10.9.9.9".to_owned())
        );
        assert_eq!(options.auth_base_url(), Some("http://10.9.9.7".to_owned()));
    }

    #[test]
    fn an_empty_project_id_is_invalid() {
        let error = FirebaseOptions::default().validate().unwrap_err();
        assert_eq!(error.code, crate::error::INVALID_ARGUMENT);
    }
}
