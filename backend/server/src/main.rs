//! STUB: lo reemplaza el agente del server.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let app = axum::Router::new().route("/api/health", axum::routing::get(|| async { "ok" }));
    let l = tokio::net::TcpListener::bind("127.0.0.1:7700").await?;
    axum::serve(l, app).await?;
    Ok(())
}
