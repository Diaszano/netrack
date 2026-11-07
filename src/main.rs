use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context;
use netrack::api::{AppState, router};
use netrack::db::Db;
use netrack::db::tests_repo::TestKind;
use netrack::runner::{RunError, Runner};
use netrack::scheduler::Scheduler;
use netrack::settings::Settings;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    let settings = Settings::from_env()?;
    tracing::info!(
        port = settings.port,
        data_dir = %settings.data_dir.display(),
        tz = %settings.tz,
        run_on_startup = settings.run_on_startup,
        "netrack starting"
    );

    std::fs::create_dir_all(&settings.data_dir)
        .with_context(|| format!("cannot create {}", settings.data_dir.display()))?;
    let db = Db::open(&settings.data_dir.join("netrack.db"), settings.tz).await?;
    let runner = Runner::with_default_providers(db.clone());
    let (reload_tx, reload_rx) = watch::channel(());
    let scheduler = Arc::new(Scheduler::spawn(db.clone(), runner.clone(), reload_rx));

    if settings.run_on_startup {
        let runner = runner.clone();
        tokio::spawn(async move {
            match runner.run(TestKind::Auto).await {
                Ok(_) => {}
                Err(RunError::Busy) => tracing::info!("startup test skipped: already running"),
                Err(e) => tracing::error!(error = %e, "startup test failed"),
            }
        });
    }

    // `state` (and its reload sender) lives until the servers have stopped.
    let state = AppState::new(db, runner, reload_tx, scheduler);
    let app = router(state);

    let v4 = TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], settings.port)))
        .await
        .with_context(|| format!("cannot bind 0.0.0.0:{}", settings.port))?;
    tracing::info!(addr = %v4.local_addr()?, "listening");
    let v6 = match TcpListener::bind(SocketAddr::from((
        std::net::Ipv6Addr::UNSPECIFIED,
        settings.port,
    )))
    .await
    {
        Ok(l) => {
            tracing::info!(addr = %l.local_addr()?, "listening");
            Some(l)
        }
        Err(e) => {
            tracing::warn!(error = %e, "cannot bind [::]; serving IPv4 only");
            None
        }
    };

    let (stop_tx, stop_rx) = watch::channel(false);
    tokio::spawn(async move {
        shutdown_signal().await;
        tracing::info!("shutting down");
        let _ = stop_tx.send(true);
    });

    let serve = |listener: TcpListener| {
        let app = app.clone();
        let mut stop = stop_rx.clone();
        async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    let _ = stop.wait_for(|s| *s).await;
                })
                .await
        }
    };
    match v6 {
        Some(v6) => {
            let (a, b) = tokio::join!(serve(v4), serve(v6));
            a?;
            b?;
        }
        None => serve(v4).await?,
    }
    drop(app);
    Ok(())
}

async fn shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};
    let mut term = signal(SignalKind::terminate()).expect("install SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
}
