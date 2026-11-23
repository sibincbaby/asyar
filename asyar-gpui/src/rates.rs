//! Currency rates for the calculator, shared with the Tauri app through
//! `<data dir>/calculator/rates.json`.
//!
//! Queries never wait on the network: they use whatever rates are loaded, and
//! a stale or missing cache triggers one background refresh.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use asyar_calculator::currency::{RatesCache, fetch_rates};

#[derive(Default)]
pub struct Rates {
    cache: Mutex<Option<RatesCache>>,
    fetching: AtomicBool,
}

/// Rates map plus its human-readable age, as `EvalContext` takes them.
pub type Snapshot = (Option<Arc<HashMap<String, f64>>>, Option<String>);

impl Rates {
    pub fn snapshot(&self) -> Snapshot {
        let now = SystemTime::now();
        match &*self.cache.lock().unwrap() {
            Some(cache) => (
                Some(Arc::new(cache.rates.clone())),
                Some(cache.age_human(now)),
            ),
            None => (None, None),
        }
    }

    /// Re-reads the shared cache (the Tauri app may have refreshed it) and, if
    /// it is still missing or older than `ttl_hours`, fetches in the background.
    pub fn ensure_fresh(self: &Arc<Self>, ttl_hours: f64) {
        let Some(path) = rates_path() else { return };
        let now = SystemTime::now();
        let stale = {
            let mut cache = self.cache.lock().unwrap();
            if let Some(disk) = RatesCache::load(&path)
                && cache
                    .as_ref()
                    .is_none_or(|c| disk.fetched_at_epoch_secs > c.fetched_at_epoch_secs)
            {
                *cache = Some(disk);
            }
            cache.as_ref().is_none_or(|c| c.is_stale(ttl_hours, now))
        };
        if !stale || self.fetching.swap(true, Ordering::SeqCst) {
            return;
        }
        let this = self.clone();
        // reqwest needs a Tokio reactor, which GPUI's executor is not.
        std::thread::spawn(move || {
            let fetched = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| e.to_string())
                .and_then(|rt| rt.block_on(fetch_rates()));
            match fetched {
                Ok(rates) => {
                    let cache = RatesCache::new(rates, SystemTime::now());
                    if let Err(error) = cache.save(&path) {
                        eprintln!("[asyar-gpui] could not save rates: {error}");
                    }
                    *this.cache.lock().unwrap() = Some(cache);
                }
                Err(error) => eprintln!("[asyar-gpui] rates refresh failed: {error}"),
            }
            this.fetching.store(false, Ordering::SeqCst);
        });
    }
}

fn rates_path() -> Option<PathBuf> {
    crate::settings::data_dir().map(|dir| dir.join("calculator").join("rates.json"))
}
