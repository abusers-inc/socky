#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Input output failed")]
    IO {
        #[from]
        source: std::io::Error,
    },
    #[error("async-tungstenite failed performing operation")]
    TungsteniteFailure {
        #[from]
        source: async_tungstenite::tungstenite::Error,
    },

    #[error("Failed connecting to endpoint")]
    Connection(#[from] ConnectionError),
}

#[derive(Debug, thiserror::Error)]
pub enum InvalidURL {
    #[error("missing wss:// or ws:// scheme")]
    MissingScheme,
    #[error("missing host specifier")]
    MissingHost,
}

#[derive(thiserror::Error, Debug)]
pub enum ConnectionError {
    #[error("Can't deduce connection port: nor it is present in request, nor default port for scheme is recognized")]
    NoPort,
    #[error("Proxy connection failed")]
    ProxyTunnel {
        #[from]
        soruce: proxied::ConnectError,
    },
    #[error("async-tungstenite failed connecting")]
    Tungstenite {
        #[from]
        source: async_tungstenite::tungstenite::Error,
    },

    #[error("TLS tunnel can't be established")]
    Tls(#[from] TlsError),

    #[error("This URL is invalid: {0}")]
    URL(#[from] InvalidURL),
}

#[derive(thiserror::Error, Debug)]
pub enum TlsError {
    #[error("Failed parsing this domain: {domain}")]
    DomainParsing { domain: String },

    #[error("I/O Error during connection")]
    Connection(#[from] std::io::Error),
}
