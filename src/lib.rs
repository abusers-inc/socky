use async_tungstenite::{stream::Stream, tokio::TokioAdapter, WebSocketStream};
use errors::{ConnectionError, Error, InvalidURL, TlsError};
use futures_util::{Sink, SinkExt, StreamExt};
use proxied::Proxy;
use rustls::{client::WebPkiServerVerifier, ClientConfig, RootCertStore};
use rustls_pki_types::{DnsName, ServerName};
use std::{pin::pin, sync::Arc};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
};
use tokio_rustls::{client::TlsStream, TlsConnector};

type StreamWsProxy = WebSocketStream<TokioAdapter<TcpStream>>;
type StreamWsProxyTls = WebSocketStream<TokioAdapter<TlsStream<TcpStream>>>;

type StreamNoProxy =
    WebSocketStream<Stream<TokioAdapter<TcpStream>, TokioAdapter<TlsStream<TcpStream>>>>;

pub use async_tungstenite::tungstenite;
pub use tungstenite::{
    client::IntoClientRequest, handshake::client::Request, ClientRequestBuilder, Message,
};

pub mod errors;

/// External enum to hide implementation details
enum WebSocketConnection {
    ProxiedTls(StreamWsProxyTls),
    Proxied(StreamWsProxy),
    NotProxied(StreamNoProxy),
}

macro_rules! match_call {
    ($self_i:expr; $as:ident; $code:expr) => {
        match $self_i {
            WebSocketConnection::ProxiedTls($as) => $code,
            WebSocketConnection::Proxied($as) => $code,
            WebSocketConnection::NotProxied($as) => $code,
        }
    };
}

pub struct WebSocket(WebSocketConnection);
impl WebSocket {
    /// Create socket with proxy, allows for more granular control over Domain/Port
    async fn new_proxy(
        proxy: Proxy,
        domain: String,
        port: u16,
        request: Request,
        use_tls: bool,
    ) -> Result<Self, ConnectionError> {
        let stream = proxy
            .connect_tcp(proxied::NetworkTarget::Domain {
                domain: domain.clone(),
                port,
            })
            .await?;

        match use_tls {
            false => {
                let (client, _) = async_tungstenite::tokio::client_async(request, stream).await?;

                Ok(Self(WebSocketConnection::Proxied(client)))
            }
            true => {
                let tls = connect_tls(domain, stream).await?;

                let (client, _) = async_tungstenite::tokio::client_async(request, tls).await?;

                Ok(Self(WebSocketConnection::ProxiedTls(client)))
            }
        }
    }

    /// Creates socket without using proxy
    async fn new_no_proxy(request: Request, use_tls: bool) -> Result<Self, ConnectionError> {
        let socket = async_tungstenite::tokio::connect_async(request).await?;

        // TODO: handle no_tls option

        Ok(Self(WebSocketConnection::NotProxied(socket.0)))
    }
}

impl Sink<Message> for WebSocket {
    type Error = errors::Error;

    fn poll_ready(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        match &mut (self.as_mut().0) {
            WebSocketConnection::ProxiedTls(web_socket_stream) => {
                let inner = pin!(web_socket_stream);

                <_ as Sink<Message>>::poll_ready(inner, cx)
                    .map_err(|e| errors::Error::TungsteniteFailure { source: e })
            }
            WebSocketConnection::Proxied(web_socket_stream) => {
                let inner = pin!(web_socket_stream);

                <_ as Sink<Message>>::poll_ready(inner, cx)
                    .map_err(|e| errors::Error::TungsteniteFailure { source: e })
            }
            WebSocketConnection::NotProxied(web_socket_stream) => {
                let inner = pin!(web_socket_stream);

                <_ as Sink<Message>>::poll_ready(inner, cx)
                    .map_err(|e| errors::Error::TungsteniteFailure { source: e })
            }
        }
    }

    fn start_send(mut self: std::pin::Pin<&mut Self>, item: Message) -> Result<(), Self::Error> {
        match &mut (self.as_mut().0) {
            WebSocketConnection::ProxiedTls(web_socket_stream) => {
                let inner = pin!(web_socket_stream);

                <_ as Sink<Message>>::start_send(inner, item)
                    .map_err(|e| errors::Error::TungsteniteFailure { source: e })
            }
            WebSocketConnection::Proxied(web_socket_stream) => {
                let inner = pin!(web_socket_stream);

                <_ as Sink<Message>>::start_send(inner, item)
                    .map_err(|e| errors::Error::TungsteniteFailure { source: e })
            }
            WebSocketConnection::NotProxied(web_socket_stream) => {
                let inner = pin!(web_socket_stream);

                <_ as Sink<Message>>::start_send(inner, item)
                    .map_err(|e| errors::Error::TungsteniteFailure { source: e })
            }
        }
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        match &mut (self.as_mut().0) {
            WebSocketConnection::ProxiedTls(web_socket_stream) => {
                let inner = pin!(web_socket_stream);

                <_ as Sink<Message>>::poll_flush(inner, cx)
                    .map_err(|e| errors::Error::TungsteniteFailure { source: e })
            }
            WebSocketConnection::Proxied(web_socket_stream) => {
                let inner = pin!(web_socket_stream);

                <_ as Sink<Message>>::poll_flush(inner, cx)
                    .map_err(|e| errors::Error::TungsteniteFailure { source: e })
            }
            WebSocketConnection::NotProxied(web_socket_stream) => {
                let inner = pin!(web_socket_stream);

                <_ as Sink<Message>>::poll_flush(inner, cx)
                    .map_err(|e| errors::Error::TungsteniteFailure { source: e })
            }
        }
    }

    fn poll_close(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        match &mut (self.as_mut().0) {
            WebSocketConnection::ProxiedTls(web_socket_stream) => {
                let inner = pin!(web_socket_stream);

                <_ as Sink<Message>>::poll_close(inner, cx)
                    .map_err(|e| errors::Error::TungsteniteFailure { source: e })
            }
            WebSocketConnection::Proxied(web_socket_stream) => {
                let inner = pin!(web_socket_stream);

                <_ as Sink<Message>>::poll_close(inner, cx)
                    .map_err(|e| errors::Error::TungsteniteFailure { source: e })
            }
            WebSocketConnection::NotProxied(web_socket_stream) => {
                let inner = pin!(web_socket_stream);

                <_ as Sink<Message>>::poll_close(inner, cx)
                    .map_err(|e| errors::Error::TungsteniteFailure { source: e })
            }
        }
    }
}

impl futures_util::Stream for WebSocket {
    type Item = Result<Message, errors::Error>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        match &mut (self.as_mut().0) {
            WebSocketConnection::ProxiedTls(web_socket_stream) => {
                let inner = pin!(web_socket_stream);

                <_ as futures_util::Stream>::poll_next(inner, cx)
                    .map_err(|e| errors::Error::TungsteniteFailure { source: e })
            }
            WebSocketConnection::Proxied(web_socket_stream) => {
                let inner = pin!(web_socket_stream);

                <_ as futures_util::Stream>::poll_next(inner, cx)
                    .map_err(|e| errors::Error::TungsteniteFailure { source: e })
            }
            WebSocketConnection::NotProxied(web_socket_stream) => {
                let inner = pin!(web_socket_stream);

                <_ as futures_util::Stream>::poll_next(inner, cx)
                    .map_err(|e| errors::Error::TungsteniteFailure { source: e })
            }
        }
    }
}

/// WebSocket Connection builder. You should start here
#[derive(bon::Builder)]
pub struct ConnectionRequest {
    proxy: Option<Proxy>,
    #[builder(into)]
    request: Request,

    #[builder(default = true)]
    use_tls: bool,
}

impl ConnectionRequest {
    pub async fn connect(self) -> Result<WebSocket, errors::ConnectionError> {
        match self.proxy {
            Some(proxy) => {
                let uri = self.request.uri();
                let domain = uri
                    .host()
                    .map(|x| x.to_owned())
                    .ok_or(ConnectionError::URL(InvalidURL::MissingHost.into()))?;
                let port = match uri.port_u16() {
                    None => match uri.scheme_str() {
                        Some("wss") => 443,
                        Some("ws") => 80,
                        _ => return Err(ConnectionError::URL(InvalidURL::MissingScheme.into())),
                    },
                    Some(p) => p,
                };

                WebSocket::new_proxy(proxy, domain, port, self.request, self.use_tls).await
            }
            None => WebSocket::new_no_proxy(self.request, self.use_tls).await,
        }
    }
}

async fn connect_tls<T: AsyncRead + AsyncWrite + Unpin>(
    domain: String,
    stream: T,
) -> Result<TlsStream<T>, TlsError> {
    let root_cert_store = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_owned(),
    };

    let webpki_verifier = WebPkiServerVerifier::builder(root_cert_store.into())
        .build()
        .unwrap();

    let client_config = ClientConfig::builder()
        .with_webpki_verifier(webpki_verifier)
        .with_no_client_auth();

    let connector = TlsConnector::from(Arc::new(client_config));

    let dns_name =
        DnsName::try_from(domain.clone()).map_err(|_| TlsError::DomainParsing { domain })?;

    Ok(connector
        .connect(ServerName::DnsName(dns_name), stream)
        .await?)
}
