// The demo's routes: a global every screen can push to, one page per route.

use nsh_core::{
    element::{SharedString, Signal},
    prelude::*,
};

/// A screen of the demo.
#[derive(Clone, Copy, PartialEq, Default)]
pub enum Route {
    /// The index of the products.
    #[default]
    Home,
    Auth,
    Database,
    Storage,
    Messaging,
    Analytics,
    Monitor,
}

/// The product routes, in the index's order.
pub const PRODUCTS: [Route; 6] = [
    Route::Auth,
    Route::Database,
    Route::Storage,
    Route::Messaging,
    Route::Analytics,
    Route::Monitor,
];

impl Route {
    /// The route's key in the stack (its stable identity).
    pub fn key(self) -> SharedString {
        // `From<&'static str>`: no allocation on the each() key path.
        SharedString::from(match self {
            Route::Home => "route:home",
            Route::Auth => "route:auth",
            Route::Database => "route:database",
            Route::Storage => "route:storage",
            Route::Messaging => "route:messaging",
            Route::Analytics => "route:analytics",
            Route::Monitor => "route:monitor",
        })
    }

    /// The page's test id (`id:` in tests).
    pub fn test_id(self) -> &'static str {
        match self {
            Route::Home => "page-home",
            Route::Auth => "page-auth",
            Route::Database => "page-database",
            Route::Storage => "page-storage",
            Route::Messaging => "page-messaging",
            Route::Analytics => "page-analytics",
            Route::Monitor => "page-monitor",
        }
    }

    /// The route's title: the system header's and the index row's.
    pub fn title(self) -> &'static str {
        match self {
            Route::Home => "Firebase demo",
            Route::Auth => "Authentication",
            Route::Database => "Realtime Database",
            Route::Storage => "Cloud Storage",
            Route::Messaging => "Cloud Messaging",
            Route::Analytics => "Analytics",
            Route::Monitor => "Performance",
        }
    }

    /// One line of what the route will show, for the index row.
    pub fn detail(self) -> &'static str {
        match self {
            Route::Home => "",
            Route::Auth => "Email and anonymous sign-in, tokens, users",
            Route::Database => "Reads, writes and live listeners",
            Route::Storage => "Uploads with progress, downloads, metadata",
            Route::Messaging => "Tokens, topics, received messages",
            Route::Analytics => "Events, screen views, user properties",
            Route::Monitor => "Traces and per-call network timings",
        }
    }
}

/// The pushed routes, bottom to top.
#[derive(Default, Clone, PartialEq)]
pub struct Routes(pub(crate) Vec<Route>);

/// The pushed routes, a signal of the surface's.
pub fn routes() -> Signal<Routes> {
    global::<Routes>()
}

/// How many routes are pushed over the index.
pub fn depth() -> usize {
    routes().with_untracked(|routes| routes.0.len())
}

/// Pushes `route`; a route already in the stack is not pushed twice.
pub fn push(route: Route) {
    routes().update(|routes| {
        if !routes.0.contains(&route) {
            routes.0.push(route);
        }
    });
}

/// Pops the top route.
pub fn pop() {
    routes().update(|routes| {
        routes.0.pop();
    });
}

/// Drops the routes from `depth` up (the user popped a page natively).
pub fn truncate(depth: usize) {
    routes().update(|routes| {
        routes.0.truncate(depth);
    });
}
