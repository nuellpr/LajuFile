use std::net::SocketAddr;

use axum::{
    Router,
    body::Body,
    http::{StatusCode, header},
    routing::get,
};

pub struct TestServer {
    pub base: String,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

pub fn start_server() -> TestServer {
    let (addr_tx, addr_rx) = std::sync::mpsc::channel();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

    let join = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind test server");
            let addr = listener.local_addr().expect("test server address");
            addr_tx.send(addr).expect("send test server address");
            axum::serve(listener, app())
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .expect("serve test server");
        });
    });

    let addr: SocketAddr = addr_rx.recv().expect("test server address");
    TestServer {
        base: format!("http://{addr}"),
        shutdown: Some(shutdown_tx),
        join: Some(join),
    }
}

fn app() -> Router {
    Router::new()
        .route("/file.bin", get(file))
        .route("/missing", get(|| async { StatusCode::NOT_FOUND }))
}

async fn file() -> impl axum::response::IntoResponse {
    let body = b"LajuFile".repeat(1024 * 128);
    (
        [
            (header::CONTENT_TYPE, "application/octet-stream"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"file.bin\"",
            ),
        ],
        Body::from(body),
    )
}
