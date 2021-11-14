use std::net::SocketAddr;
use std::str::FromStr;

use futures::StreamExt;
use hyper::{Body, Request, Response, Server, Uri};
use hyper::service::{make_service_fn, service_fn};
use log::{debug,info, trace};
use serde::Deserialize;

use zip::ZipWriter;

mod zip;

#[derive(Clone, Debug, Deserialize)]
struct ZipRequestEntry {
    url: String,
    filename: String,
}

type ZipRequest = Vec<ZipRequestEntry>;

async fn zip_request_handler(req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
    let bytes = hyper::body::to_bytes(req).await?;

    if let Ok(zip_request) = serde_json::from_slice::<ZipRequest>(&bytes) {
        debug!("handling request");
        let (sender, body) = Body::channel();

        tokio::spawn(async move {
            let mut zip = ZipWriter::new(sender);

            for entry in zip_request {
                debug!("downloading file {}", entry.filename);
                zip.start_file(&entry.filename).await.unwrap();

                let uri = Uri::from_str(&entry.url).unwrap();
                let https = hyper_tls::HttpsConnector::new();
                let client = hyper::client::Client::builder()
                    .build::<_, hyper::Body>(https);

                let mut res = client.get(uri).await.unwrap();
                let body = res.body_mut();

                debug!("writing file {}", entry.filename);
                while let Some(buf) = body.next().await {
                    trace!("writing buffer");
                    zip.write(&buf.unwrap()).await.unwrap();
                }

                debug!("finished writing {}", entry.filename);
                zip.finish_file().await.unwrap();
            }

            debug!("finished writing all files");
            let _ = zip.finish().await;
        });

        let response = Response::builder()
            .header("content-type", "application/zip")
            .header("content-disposition", "attachment; filename=\"archive.zip\"")
            .body(body)
            .unwrap();

        return Ok(response);
    }

    let bad_request = Response::builder()
        .status(400)
        .body(Body::from("unable to parse json"))
        .unwrap();

    return Ok(bad_request);
}

#[tokio::main]
async fn main() -> tokio::io::Result<()> {
    let logger = simple_logger::SimpleLogger::new();
    // TODO make log level configurable
    async_log::Logger::wrap(logger, || 0)
        .start(log::LevelFilter::Debug)
        .unwrap();

    // TODO make this configurable
    info!{"starting server on 127.0.0.1:3000"}
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));

    let make_zip_service = make_service_fn(|_conn| async {
        Ok::<_, hyper::Error>(service_fn(zip_request_handler))
    });

    let server = Server::bind(&addr)
        .serve(make_zip_service);

    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
    }

    Ok(())
}













