use anyhow::Result;
use std::convert::Infallible;
use std::net::SocketAddr;

use http_body_util::Full;
use hyper::{body, header, server, service, Request, Response};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;

use prometheus::{Encoder, TextEncoder};

pub async fn gather_encode(
    _: Request<body::Incoming>,
) -> Result<Response<Full<body::Bytes>>, Infallible> {
    let encoder = TextEncoder::new();

    let metric_families = prometheus::gather();
    let mut buffer = Vec::with_capacity(2_usize.pow(12));
    encoder.encode(&metric_families, &mut buffer).unwrap();

    let response = Response::builder()
        .status(200)
        .header(header::CONTENT_TYPE, encoder.format_type())
        .body(Full::new(body::Bytes::from(buffer)))
        .unwrap();

    Ok(response)
}

pub async fn start_prometheus_listener_task(addr: SocketAddr) -> Result<()> {
    println!("PROMETH: listening: {}", addr);

    let listener = TcpListener::bind(addr).await?;

    tokio::task::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _addr)) => {
                    // println!("PROMETH: received: {}", _addr);
                    let io = TokioIo::new(stream);

                    tokio::task::spawn(async move {
                        if let Err(err) = server::conn::http1::Builder::new()
                            .serve_connection(io, service::service_fn(gather_encode))
                            .await
                        {
                            println!("PROMETH: serving connection: {:?}", err);
                        }
                    });
                }
                Err(e) => println!("PROMETH: HTTP: {}", e),
            };
        }
    });

    Ok(())
}
