//! The Worker's binding: `WebHost.open(db)` installs SQLite's OPFS VFS as the default and opens
//! the store and the host; `call(cmd, argsJson)` answers one command with its JSON result;
//! `takeChanges()` hands over the change events for the page. Only a dedicated Worker can open
//! OPFS sync access handles, so this runs there, never in the page.
//! Refs: <https://docs.rs/wasm-bindgen> · <https://docs.rs/sqlite-wasm-vfs> ·
//! <https://developer.mozilla.org/en-US/docs/Web/API/FileSystemFileHandle/createSyncAccessHandle>

use std::path::Path;

use serde_json::Value;
use sqlite_wasm_vfs::sahpool::{OpfsSAHPoolCfg, install};
use txtodo_store::Store;
use wasm_bindgen::prelude::*;

use crate::host::Host;

fn js(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
}

/// The host, held by the Worker.
#[wasm_bindgen]
pub struct WebHost {
    host: Host,
}

#[wasm_bindgen]
impl WebHost {
    /// Opens `db` (a path in the OPFS pool, e.g. `/txtodo.db`); creates it the first time.
    pub async fn open(db: String) -> Result<WebHost, JsValue> {
        // Default VFS: `Store::open` needs no change. A second call reuses the registered pool.
        install::<sqlite_wasm_rs::WasmOsCallback>(&OpfsSAHPoolCfg::default(), true)
            .await
            .map_err(js)?;
        let store = Store::open(Path::new(&db)).map_err(js)?;
        Ok(WebHost {
            host: Host::open(store).map_err(js)?,
        })
    }

    /// This device's id, ULID text.
    pub fn device(&self) -> String {
        self.host.device().to_string()
    }

    /// One command; `args` and the result are JSON text. An error is the message the page shows.
    pub fn call(&mut self, cmd: &str, args: &str) -> Result<String, JsValue> {
        let args: Value = if args.is_empty() {
            Value::Null
        } else {
            serde_json::from_str(args).map_err(js)?
        };
        let out = self.host.call(cmd, &args).map_err(js)?;
        serde_json::to_string(&out).map_err(js)
    }

    /// The change events since the last call, a JSON array of `ChangeDto`.
    #[wasm_bindgen(js_name = takeChanges)]
    pub fn take_changes(&mut self) -> Result<String, JsValue> {
        serde_json::to_string(&self.host.take_changes()).map_err(js)
    }
}
