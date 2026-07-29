use tokio::{signal, sync::mpsc};
use tonic::transport::Server;
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
        println!("Shutdown signal recieved, starting graceful shutdown");
    };
    println!("Engine gRPC server listening on {}", address);
    Server::builder()
        .add_service(UserSerivcesServer::new(service.clone()))
        .add_service(EngineServicesServer::new(service))
        .serve_with_shutdown(address, shutdown_signal)
        .await?;
    println!("Engine Server safely terminated");
    Ok(())
}
