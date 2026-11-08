#![deny(warnings)]

pub mod aof;
pub mod cluster;
pub mod db;
pub mod network;
pub mod parser;
pub mod pubsub;
pub mod replication;
pub mod security;
pub mod studio_api;

use mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

fn resolve_bind_addr() -> String {
    if let Some(arg) = std::env::args().nth(1) {
        if !arg.contains(':') {
            return format!("0.0.0.0:{arg}");
        }
        return arg;
    }
    if let Ok(port) = std::env::var("PORT") {
        return format!("0.0.0.0:{port}");
    }
    if let Ok(addr) = std::env::var("VELOCITY_BIND_ADDR") {
        return addr;
    }
    "0.0.0.0:6379".to_string()
}

fn resolve_studio_addr() -> String {
    std::env::var("STUDIO_BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string())
}

#[cfg(target_os = "linux")]
fn main() {
    let bind_addr = resolve_bind_addr();
    let studio_addr = resolve_studio_addr();

    // Spawn Studio HTTP & WebSocket Management Server on background Tokio thread
    let telemetry = std::sync::Arc::new(tokio::sync::RwLock::new(studio_api::TelemetryData::default()));
    let key_store = std::sync::Arc::new(tokio::sync::RwLock::new(studio_api::StudioKeyStore::new_with_defaults()));
    if let Err(e) = studio_api::spawn_studio_http_server(&studio_addr, telemetry, key_store) {
        eprintln!("Warning: Failed to launch Studio HTTP server: {e}");
    }

    if let Err(e) = network::run_server(&bind_addr) {
        eprintln!("VelocityDB server error: {e}");
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    let bind_addr = resolve_bind_addr();
    let studio_addr = resolve_studio_addr();

    println!("VelocityDB uses io_uring (Glommio) and requires Linux (kernel >= 5.8).");

    // Spawn Studio HTTP & WebSocket Management Server on background Tokio thread
    let telemetry = std::sync::Arc::new(tokio::sync::RwLock::new(studio_api::TelemetryData::default()));
    let key_store = std::sync::Arc::new(tokio::sync::RwLock::new(studio_api::StudioKeyStore::new_with_defaults()));
    if let Err(e) = studio_api::spawn_studio_http_server(&studio_addr, telemetry, key_store) {
        eprintln!("Warning: Failed to launch Studio HTTP server: {e}");
    }

    if let Err(e) = network::run_server(&bind_addr) {
        eprintln!("VelocityDB server error: {e}");
    }
}
