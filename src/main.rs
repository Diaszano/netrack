use netrack::settings::Settings;
use tracing_subscriber::EnvFilter;

fn main() -> anyhow::Result<()> {
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
        "netrack settings loaded"
    );
    Ok(())
}
