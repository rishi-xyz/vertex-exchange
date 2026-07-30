use tokio::{signal, sync::mpsc};
use tonic::transport::Server;
use tracing_subscriber;
use vertex_engine::{
    engine::engine_from_env,
    grpc::{
        self, EngineService,
        proto::{
            engine_services_server::EngineServicesServer, user_serivces_server::UserSerivcesServer,
        },
    },
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,engine=debug"));
    tracing_subscriber::fmt().with_env_filter(env_filter).init();

    tracing::info!("Starting Vertex Engine");

    let engine = engine_from_env(1, 1);
    let (tx, rx) = mpsc::channel(100);
    tokio::spawn(grpc::run_engine(rx, engine));
    let service = EngineService::new(tx);
    let port = std::env::var("PORT").unwrap_or_else(|_| ("5000").into());
    let address = format!("0.0.0.0:{}", port).parse()?;
    let shutdown_signal = async {
        signal::ctrl_c()
            .await
            .expect("Failed to listen for shutdown signal");
        tracing::info!("Shutdown signal received, starting graceful shutdown");
    };
    tracing::info!("Engine gRPC server listening on {}", address);
    Server::builder()
        .add_service(UserSerivcesServer::new(service.clone()))
        .add_service(EngineServicesServer::new(service))
        .serve_with_shutdown(address, shutdown_signal)
        .await?;
    tracing::info!("Engine Server safely terminated");
    Ok(())
}
