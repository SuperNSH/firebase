//! The app registry: `initializeApp` / `get_app`, mirroring the
//! TypeScript side (packages/nsh-firebase/src/core/app.ts).

use std::sync::{Arc, Mutex, OnceLock};

use crate::{bus::EventBus, error::FirebaseError, options::FirebaseOptions};

/// One initialized app: its options, its event bus, and the native tier
/// the products reach through [`crate::host::FirebaseHost`].
pub struct FirebaseApp {
    pub name: String,
    pub options: FirebaseOptions,
    pub events: EventBus,
}

impl std::fmt::Debug for FirebaseApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FirebaseApp")
            .field("name", &self.name)
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

static APPS: OnceLock<Mutex<Vec<Arc<FirebaseApp>>>> = OnceLock::new();

fn registry() -> &'static Mutex<Vec<Arc<FirebaseApp>>> {
    APPS.get_or_init(|| Mutex::new(Vec::new()))
}

/// Initializes (or returns) the app named `name`. Initializing an
/// existing name returns the same app: options are only read once.
pub fn initialize_app(
    options: FirebaseOptions,
    name: &str,
) -> Result<Arc<FirebaseApp>, FirebaseError> {
    options.validate()?;
    let mut apps = registry().lock().expect("app registry lock");
    if let Some(existing) = apps.iter().find(|app| app.name == name) {
        return Ok(Arc::clone(existing));
    }
    let app = Arc::new(FirebaseApp {
        name: name.to_owned(),
        options,
        events: EventBus::new(),
    });
    apps.push(Arc::clone(&app));
    Ok(app)
}

/// The app named `name`, `"[DEFAULT]"` unless said otherwise.
pub fn get_app(name: &str) -> Result<Arc<FirebaseApp>, FirebaseError> {
    let apps = registry().lock().expect("app registry lock");
    apps.iter()
        .find(|app| app.name == name)
        .map(Arc::clone)
        .ok_or_else(|| {
            FirebaseError::new(
                crate::error::NO_APP,
                format!("no app named `{name}`; call initialize_app first"),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initializing_twice_returns_one_app() {
        let first = initialize_app(FirebaseOptions::demo(), "once-test").unwrap();
        let second = initialize_app(FirebaseOptions::demo(), "once-test").unwrap();
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn a_missing_app_is_a_stable_no_app_error() {
        let error = get_app("never-initialized-test").unwrap_err();
        assert_eq!(error.code, crate::error::NO_APP);
    }

    #[test]
    fn invalid_options_never_reach_the_registry() {
        let error = initialize_app(FirebaseOptions::default(), "invalid-test").unwrap_err();
        assert_eq!(error.code, crate::error::INVALID_ARGUMENT);
        assert!(get_app("invalid-test").is_err());
    }
}
