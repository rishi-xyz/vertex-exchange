use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let proto_root = format!("{}/../proto/engine", env!("CARGO_MANIFEST_DIR"));
    tonic_prost_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(
            &[format!("{}/vertex_engine.proto", proto_root)],
            &[proto_root],
        )?;
    Ok(())
}
