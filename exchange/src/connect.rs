use std::sync::{Arc, LazyLock};

use crate::adapter::AdapterError;
use bytes::Bytes;
use fastwebsockets::FragmentCollector;
use http_body_util::Empty;
use hyper::{
    Request,
    header::{CONNECTION, UPGRADE},
    upgrade::Upgraded,
};
use hyper_util::rt::TokioIo;
use tokio::net::TcpStream;
use tokio_rustls::{
    TlsConnector,
    rustls::{ClientConfig, pki_types::ServerName},
};

#[allow(clippy::large_enum_variant)]
pub enum State {
    Disconnected,
    Connected(FragmentCollector<TokioIo<Upgraded>>),
}

pub use fastwebsockets::{FragmentCollectorRead, WebSocketWrite};
use tokio::io::{ReadHalf, WriteHalf};

pub type WsReader = FragmentCollectorRead<ReadHalf<TokioIo<Upgraded>>>;
pub type WsWriter = WebSocketWrite<WriteHalf<TokioIo<Upgraded>>>;

pub(crate) static TLS_CONNECTOR: LazyLock<TlsConnector> = LazyLock::new(|| {
    let root_store = tokio_rustls::rustls::RootCertStore::from_iter(
        webpki_roots::TLS_SERVER_ROOTS.iter().cloned(),
    );

    let config = ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    TlsConnector::from(Arc::new(config))
});

pub async fn connect_ws(
    domain: &str,
    url: &str,
) -> Result<
    fastwebsockets::FragmentCollector<hyper_util::rt::TokioIo<hyper::upgrade::Upgraded>>,
    AdapterError,
> {
    let tcp_stream = setup_tcp(domain).await?;
    let tls_stream = upgrade_to_tls(domain, tcp_stream).await?;

    upgrade_to_websocket(domain, tls_stream, url).await
}

pub async fn connect_ws_split(
    domain: &str,
    url: &str,
) -> Result<(WsReader, WsWriter), AdapterError> {
    let tcp_stream = setup_tcp(domain).await?;
    let tls_stream = upgrade_to_tls(domain, tcp_stream).await?;

    let req: Request<Empty<Bytes>> = Request::builder()
        .method("GET")
        .uri(url)
        .header("Host", domain)
        .header(UPGRADE, "websocket")
        .header(CONNECTION, "upgrade")
        .header(
            "Sec-WebSocket-Key",
            fastwebsockets::handshake::generate_key(),
        )
        .header("Sec-WebSocket-Version", "13")
        .body(Empty::<Bytes>::new())
        .map_err(|e| AdapterError::WebsocketError(e.to_string()))?;

    let (ws, _) = fastwebsockets::handshake::client(&SpawnExecutor, req, tls_stream)
        .await
        .map_err(|e| AdapterError::WebsocketError(e.to_string()))?;

    let (mut ws_reader, writer) = ws.split(tokio::io::split);
    ws_reader.set_auto_pong(false);
    ws_reader.set_auto_close(false);
    let reader = FragmentCollectorRead::new(ws_reader);
    Ok((reader, writer))
}

struct SpawnExecutor;

impl<Fut> hyper::rt::Executor<Fut> for SpawnExecutor
where
    Fut: std::future::Future + Send + 'static,
    Fut::Output: Send + 'static,
{
    fn execute(&self, fut: Fut) {
        tokio::task::spawn(fut);
    }
}

async fn setup_tcp(domain: &str) -> Result<TcpStream, AdapterError> {
    let addr = format!("{domain}:443");
    TcpStream::connect(&addr)
        .await
        .map_err(|e| AdapterError::WebsocketError(e.to_string()))
}

async fn upgrade_to_tls(
    domain: &str,
    tcp_stream: TcpStream,
) -> Result<tokio_rustls::client::TlsStream<TcpStream>, AdapterError> {
    let server_name = ServerName::try_from(domain.to_owned())
        .map_err(|_| AdapterError::ParseError("invalid dnsname".to_string()))?;

    TLS_CONNECTOR
        .connect(server_name, tcp_stream)
        .await
        .map_err(|e| AdapterError::WebsocketError(e.to_string()))
}

async fn upgrade_to_websocket(
    domain: &str,
    tls_stream: tokio_rustls::client::TlsStream<TcpStream>,
    url: &str,
) -> Result<FragmentCollector<TokioIo<Upgraded>>, AdapterError> {
    let req: Request<Empty<Bytes>> = Request::builder()
        .method("GET")
        .uri(url)
        .header("Host", domain)
        .header(UPGRADE, "websocket")
        .header(CONNECTION, "upgrade")
        .header(
            "Sec-WebSocket-Key",
            fastwebsockets::handshake::generate_key(),
        )
        .header("Sec-WebSocket-Version", "13")
        .body(Empty::<Bytes>::new())
        .map_err(|e| AdapterError::WebsocketError(e.to_string()))?;

    let (ws, _) = fastwebsockets::handshake::client(&SpawnExecutor, req, tls_stream)
        .await
        .map_err(|e| AdapterError::WebsocketError(e.to_string()))?;

    Ok(FragmentCollector::new(ws))
}
