use proxied::{Proxy, TCPConnection};
use rustls::{Certificate, OwnedTrustAnchor};

use std::sync::Arc;

use async_tungstenite::{
    stream::Stream,
    tokio::TokioAdapter,
    tungstenite::{self, Message},
    WebSocketStream,
};
use futures_util::{SinkExt, StreamExt};

use tokio::io::{AsyncRead, AsyncWrite};

use tokio_rustls::TlsConnector;

type StreamWsProxy = WebSocketStream<TokioAdapter<TCPConnection>>;
type StreamWsProxyTls =
    WebSocketStream<TokioAdapter<tokio_rustls::client::TlsStream<TCPConnection>>>;

type StreamNoProxy = WebSocketStream<
    Stream<
        TokioAdapter<tokio::net::TcpStream>,
        TokioAdapter<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>,
    >,
>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Proxy connection failed")]
    ProxyConnect {
        #[from]
        soruce: proxied::ConnectError,
    },

    #[error("Input output failed")]
    IO {
        #[from]
        source: std::io::Error,
    },

    #[error("async-tungstenite failed connecting")]
    Tungstenite {
        #[from]
        source: async_tungstenite::tungstenite::Error,
    },

    #[error("proxy address parse failed")]
    UrlParse {
        #[from]
        source: url::ParseError,
    },

    #[error("proxy host is not present")]
    HostIsNotPresent,

    #[error("Can't deduce connection port: nor it is present in request, nor default port for scheme is recognized")]
    NoPort,
}

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
    pub async fn new_proxy(
        proxy: Proxy,
        domain: String,
        port: u16,
        request: tungstenite::handshake::client::Request,
        no_tls: bool,
    ) -> Result<Self, Error> {
        let stream = proxy
            .connect_tcp(proxied::NetworkTarget::Domain {
                domain: domain.clone(),
                port,
            })
            .await?;

        match no_tls {
            true => {
                let (client, _) = async_tungstenite::tokio::client_async(request, stream).await?;

                Ok(Self(WebSocketConnection::Proxied(client)))
            }
            false => {
                let tls = connect_tls(domain, stream).await?;

                let (client, _) = async_tungstenite::tokio::client_async(request, tls).await?;

                Ok(Self(WebSocketConnection::ProxiedTls(client)))
            }
        }
    }

    /// Creates socket without using proxy
    pub async fn new_no_proxy(
        request: tungstenite::handshake::client::Request,
        no_tls: bool,
    ) -> Result<Self, Error> {
        let socket = async_tungstenite::tokio::connect_async(request).await?;

        // TODO: handle no_tls option

        Ok(Self(WebSocketConnection::NotProxied(socket.0)))
    }

    pub async fn new(
        request: impl Into<tungstenite::handshake::client::Request>,
        proxy: Option<Proxy>,
        no_tls: bool,
    ) -> Result<Self, Error> {
        let request = request.into();

        match proxy {
            Some(proxy) => {
                let uri = request.uri();
                let domain = uri
                    .host()
                    .map(|x| x.to_owned())
                    .ok_or(Error::HostIsNotPresent)?;
                let port = match uri.port_u16() {
                    None => match uri.scheme_str() {
                        Some(scheme) if scheme == "wss" => 443,
                        Some(scheme) if scheme == "ws" => 80,
                        _ => return Err(Error::NoPort),
                    },
                    Some(p) => p,
                };

                Self::new_proxy(proxy, domain, port, request, no_tls).await
            }
            None => Self::new_no_proxy(request, no_tls).await,
        }
    }

    pub async fn send(
        &mut self,
        msg: Message,
    ) -> Result<(), async_tungstenite::tungstenite::Error> {
        match_call!(&mut self.0; ws; ws.send(msg).await)
    }
    pub async fn flush(&mut self) -> Result<(), async_tungstenite::tungstenite::Error> {
        match_call!(&mut self.0; ws; ws.flush().await)
    }

    pub async fn next(
        &mut self,
    ) -> Option<
        Result<async_tungstenite::tungstenite::Message, async_tungstenite::tungstenite::Error>,
    > {
        match_call!(&mut self.0; ws; ws.next().await)
    }
}

async fn connect_tls<T: AsyncRead + AsyncWrite + Unpin>(
    domain: String,
    stream: T,
) -> Result<tokio_rustls::client::TlsStream<T>, std::io::Error> {
    let mut root_store = rustls::RootCertStore::empty();
    let roots = webpki_roots::TLS_SERVER_ROOTS.into_iter().map(|x| {
        OwnedTrustAnchor::from_subject_spki_name_constraints(
            x.subject.to_vec(),
            x.subject_public_key_info.to_vec(),
            x.name_constraints.as_ref().map(|x| x.to_vec()),
        )
    });

    let certs = rustls_native_certs::load_native_certs().ok();

    if let Some(certs) = certs {
        for cert in certs {
            root_store.add(&Certificate(cert.to_vec()));
        }
    }

    root_store.add_trust_anchors(roots);
    let config = rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_root_certificates(Arc::new(root_store))
        .with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(config));

    Ok(connector
        .connect(
            rustls::ServerName::try_from(domain.as_str()).unwrap(),
            stream,
        )
        .await?)
}
