//! Desktop shell for ExpiryScalping. The UI talks to the engine over its
//! WebSocket API; this crate only hosts the window and keeps the device
//! token in the OS credential store (Windows Credential Manager).

use tracing::{error, info};

const SERVICE: &str = "expiryscalping";
const ACCOUNT: &str = "engine-device-token";

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, ACCOUNT).map_err(|e| e.to_string())
}

/// Returns the saved token, or `None` when there is none.
#[tauri::command]
fn token_get() -> Result<Option<String>, String> {
    match entry()?.get_password() {
        Ok(t) => Ok(Some(t)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => {
            error!(%e, "keyring read failed");
            Err(e.to_string())
        }
    }
}

#[tauri::command]
fn token_set(token: String) -> Result<(), String> {
    let token = token.trim();
    if token.is_empty() {
        return Err("empty token".into());
    }
    entry()?.set_password(token).map_err(|e| {
        error!(%e, "keyring write failed");
        e.to_string()
    })
}

#[tauri::command]
fn token_clear() -> Result<(), String> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();
    info!(version = env!("CARGO_PKG_VERSION"), "starting desktop app");
    let result = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![token_get, token_set, token_clear])
        .run(tauri::generate_context!());
    if let Err(e) = result {
        error!(%e, "desktop app failed");
        std::process::exit(1);
    }
}
