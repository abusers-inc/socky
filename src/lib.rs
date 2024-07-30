type TokioWsNoProxy = WebSocketStream<
    async_tungstenite::stream::Stream<
        TokioAdapter<tokio::net::TcpStream>,
        TokioAdapter<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>,
    >,
>;
use proxied::{Proxy, ProxyKind};
use rustls::{Certificate, OwnedTrustAnchor};

use std::sync::Arc;

use async_tungstenite::{
    tokio::TokioAdapter,
    tungstenite::{self, Message},
    WebSocketStream,
};
use fast_socks5::client::Socks5Stream;
use futures_util::{SinkExt, StreamExt};

use tokio::io::{AsyncRead, AsyncWrite};

use tokio_rustls::TlsConnector;

type TokioWsSocks5 = WebSocketStream<
    TokioAdapter<tokio_rustls::client::TlsStream<Socks5Stream<tokio::net::TcpStream>>>,
>;

type TokioWsHttp =
    WebSocketStream<TokioAdapter<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>>;
type TokioWsSocks5NoTls = WebSocketStream<TokioAdapter<Socks5Stream<tokio::net::TcpStream>>>;

type TokioWsHttpNoTls = WebSocketStream<TokioAdapter<tokio::net::TcpStream>>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("fast_socks5 stream error")]
    Socks5 {
        #[from]
        source: fast_socks5::SocksError,
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

    #[error("http proxy failed connecting")]
    AsyncHttp {
        #[from]
        source: async_http_proxy::HttpError,
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
    NoProxy(TokioWsNoProxy),
    SocksTunnel(TokioWsSocks5),
    HttpTunnel(TokioWsHttp),
    SocksTunnelNoTls(TokioWsSocks5NoTls),
    HttpTunnelNoTls(TokioWsHttpNoTls),
}

macro_rules! match_call {
    ($self_i:expr; $as:ident; $code:expr) => {
        match $self_i {
            WebSocketConnection::NoProxy($as) => $code,

            WebSocketConnection::SocksTunnel($as) => $code,

            WebSocketConnection::HttpTunnel($as) => $code,

            WebSocketConnection::SocksTunnelNoTls($as) => $code,

            WebSocketConnection::HttpTunnelNoTls($as) => $code,
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
        match &proxy.kind {
            ProxyKind::Socks5 => {
                let proxy = connect_socks5_proxy(proxy, domain.clone(), port).await?;

                match no_tls {
                    true => {
                        let socket = async_tungstenite::tokio::client_async(request, proxy).await?;

                        Ok(Self(WebSocketConnection::SocksTunnelNoTls(socket.0)))
                    }
                    false => {
                        let tls = connect_proxy_tls(domain, proxy).await?;

                        let socket = async_tungstenite::tokio::client_async(request, tls).await?;

                        Ok(Self(WebSocketConnection::SocksTunnel(socket.0)))
                    }
                }
            }
            ProxyKind::Http => {
                let proxy = connect_http_proxy(proxy, domain.clone(), port).await?;

                match no_tls {
                    true => {
                        let socket = async_tungstenite::tokio::client_async(request, proxy).await?;

                        Ok(Self(WebSocketConnection::HttpTunnelNoTls(socket.0)))
                    }
                    false => {
                        let tls = connect_proxy_tls(domain, proxy).await?;
                        let socket = async_tungstenite::tokio::client_async(request, tls).await?;

                        Ok(Self(WebSocketConnection::HttpTunnel(socket.0)))
                    }
                }
            }
            _ => todo!(),
        }
    }

    /// Creates socket without using proxy

    pub async fn new_no_proxy(
        request: tungstenite::handshake::client::Request,
    ) -> Result<Self, Error> {
        let socket = async_tungstenite::tokio::connect_async(request).await?;

        Ok(Self(WebSocketConnection::NoProxy(socket.0)))
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
            None => Self::new_no_proxy(request).await,
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

async fn connect_http_proxy(
    proxy: Proxy,
    domain: String,
    port: u16,
) -> Result<tokio::net::TcpStream, Error> {
    let host = match proxy.addr.chars().any(char::is_alphabetic) {
        true => {
            let resolved_ip =
                tokio::net::lookup_host(format!("{}:{}", proxy.addr.to_string(), proxy.port))
                    .await?
                    .collect::<Vec<_>>();
            let resolved_ip = resolved_ip.get(0).ok_or(Error::HostIsNotPresent)?;
            resolved_ip.to_string()
        }
        false => format!("{}:{}", proxy.addr.to_string(), proxy.port),
    };

    let mut stream = tokio::net::TcpStream::connect(host).await?;

    match proxy.creds {
        Some((login, password)) => {
            async_http_proxy::http_connect_tokio_with_basic_auth(
                &mut stream,
                &domain,
                port,
                &login,
                &password,
            )
            .await?
        }

        None => async_http_proxy::http_connect_tokio(&mut stream, &domain, port).await?,
    };

    Ok(stream)
}

async fn connect_socks5_proxy(
    proxy: Proxy,
    domain: String,
    port: u16,
) -> Result<fast_socks5::client::Socks5Stream<tokio::net::TcpStream>, fast_socks5::SocksError> {
    let socks_ip = format!("{}:{}", proxy.addr, proxy.port);

    let socks;

    match proxy.creds {
        Some(creds) => {
            socks = Socks5Stream::connect_with_password(
                socks_ip,
                domain.to_owned(),
                port,
                creds.0,
                creds.1,
                fast_socks5::client::Config::default(),
            )
            .await?;
        }
        None => {
            socks = Socks5Stream::connect(
                socks_ip,
                domain.to_owned(),
                port,
                fast_socks5::client::Config::default(),
            )
            .await?;
        }
    }

    Ok(socks)
}

async fn connect_proxy_tls<T: AsyncRead + AsyncWrite + Unpin>(
    domain: String,
    proxy: T,
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
            proxy,
        )
        .await?)
}
