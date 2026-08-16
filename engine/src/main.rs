use tokio::{
    signal,
    sync::{mpsc, oneshot},
};
use tonic::transport::Server;
use vertex_engine::{
    engine::engine_from_env,
    grpc::{
        self, EngineService,
        proto::{
            engine_services_server::EngineServicesServer, user_services_server::UserServicesServer,
        },
    },
    redis::FillPublisher,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,engine=debug"));
    tracing_subscriber::fmt().with_env_filter(env_filter).init();
    tracing::info!("Starting Vertex Engine");
    let engine = engine_from_env(1, 1);
    let publisher = FillPublisher::new().await;
    let (tx, rx) = mpsc::channel(100);
    // The engine task signals this channel when it panics processing a
    // command. The process then exits (fail-fast) instead of continuing in a
    // potentially corrupt state; with the WAL enabled, state is restored from
    // the log on restart.
    let (fatal_tx, fatal_rx) = oneshot::channel::<()>();
    tokio::spawn(grpc::run_engine(rx, engine, publisher, fatal_tx));
    let service = EngineService::new(tx);
    let port = std::env::var("PORT").unwrap_or_else(|_| ("5000").into());
    let address = format!("0.0.0.0:{}", port).parse()?;
    let shutdown_signal = async {
        tokio::select! {
            _ = signal::ctrl_c() => {
                tracing::info!("Shutdown signal received, starting graceful shutdown");
            }
            _ = fatal_rx => {
                tracing::error!("Engine task panicked — shutting down (fail-fast)");
            }
        }
    };
    tracing::info!("Engine gRPC server listening on {}", address);
    Server::builder()
        .add_service(UserServicesServer::new(service.clone()))
        .add_service(EngineServicesServer::new(service))
        .serve_with_shutdown(address, shutdown_signal)
        .await?;
    tracing::info!("Engine Server safely terminated");
    Ok(())
}
