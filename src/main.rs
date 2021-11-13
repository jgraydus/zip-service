mod zip;

use std::net::SocketAddr;
use hyper::{Request, Body, Response, Server, Uri};
use hyper::service::{make_service_fn, service_fn};
use serde::Deserialize;
use std::sync::{Arc};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use std::str::FromStr;
use std::io::{Write, SeekFrom};
use hyper::body::Bytes;
use zip::ZipWriter;

#[derive(Clone, Debug, Deserialize)]
struct ZipRequestEntry {
    url: String,
    filename: String,
}

type ZipRequest = Vec<ZipRequestEntry>;

async fn zip_request_handler(req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
    let bytes = hyper::body::to_bytes(req).await?;

    if let Ok(zip_request) = serde_json::from_slice::<ZipRequest>(&bytes) {
        println!("{:?}", zip_request);

        let (sender, body) = Body::channel();

        tokio::spawn(async move {
            let mut zip = ZipWriter::new(sender);

            for entry in zip_request {
                println!("downloading {:?} from {:?}", entry.filename, entry.url);
                let uri = Uri::from_str(&entry.url).unwrap();
                let https = hyper_tls::HttpsConnector::new();
                let client = hyper::client::Client::builder()
                    .build::<_,hyper::Body>(https);
                let res = client.get(uri).await.unwrap();
                let buf = hyper::body::to_bytes(res).await.unwrap();
                println!("writing {:?}", entry.filename);
                zip.write_file(&entry.filename, &buf).await;
            }

            zip.finish().await;
        });

        let response = Response::builder()
            .header("content-type", "application/zip")
            .header("content-disposition", "attachment; filename=\"archive.zip\"")
            .body(body)
            .unwrap();

        return Ok(response)
    }

    let bad_request = Response::builder()
        .status(400)
        .body(Body::from("unable to parse json"))
        .unwrap();

    return Ok(bad_request)
}

#[tokio::main]
async fn main() -> tokio::io::Result<()> {
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000)); // TODO config

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













