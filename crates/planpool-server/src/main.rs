mod api;
mod config;
mod store;

use std::sync::Arc;
use std::time::Duration;

use crate::config::Config;
use crate::store::Store;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub store: Arc<Store>,
}

#[tokio::main]
async fn main() {
    if std::env::args().any(|arg| arg == "--env-example") {
        print!("{}", confroid::env_example::<Config>());
        return;
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "planpool=info".into()),
        )
        .init();

    let config = Config::load().unwrap_or_else(|e| {
        eprintln!("planpool: {e}");
        std::process::exit(1);
    });
    let store = Store::new(config.data_dir.clone());
    if let Err(e) = store.init().await {
        eprintln!(
            "planpool: cannot create data dir {}: {e}",
            config.data_dir.display()
        );
        std::process::exit(1);
    }

    let state = AppState {
        config: Arc::new(config),
        store: Arc::new(store),
    };

    let sweeper_store = state.store.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            match sweeper_store.sweep().await {
                Ok(0) => {}
                Ok(n) => tracing::info!("swept {n} expired plan(s)"),
                Err(e) => tracing::warn!("sweep failed: {e}"),
            }
        }
    });

    let addr = state.config.addr;
    let data_dir = state.config.data_dir.clone();
    let app = api::router(state);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| {
            eprintln!("planpool: cannot bind {addr}: {e}");
            std::process::exit(1);
        });
    tracing::info!(
        "listening on {addr}, storing plans in {}",
        data_dir.display()
    );

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server error");
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for ctrl-c");
    tracing::info!("shutting down");
}
