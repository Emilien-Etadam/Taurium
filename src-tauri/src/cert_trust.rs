use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, ClientConnection, DigitallySignedStruct, SignatureScheme};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::error::TauriumError;

/// Certificate info surfaced to the settings UI so the user can confirm the
/// fingerprint before it gets installed into the system trust store (TOFU,
/// like an SSH host-key prompt).
#[derive(Debug, Clone, Serialize)]
pub struct CertInfo {
    pub host: String,
    pub port: u16,
    pub fingerprint: String,
}

/// Accepts any server certificate without validating it. This is used ONLY to
/// retrieve the certificate a host presents so it can be shown to the user for
/// confirmation — never to protect an actual data exchange. The connection
/// opened with this verifier must not be used to send/receive anything
/// sensitive.
#[derive(Debug)]
struct AcceptAnyCert {
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for AcceptAnyCert {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

fn parse_host_port(url: &str) -> Result<(String, u16), TauriumError> {
    let parsed = url::Url::parse(url)
        .map_err(|e| TauriumError::Certificate(format!("URL invalide : {e}")))?;
    if parsed.scheme() != "https" {
        return Err(TauriumError::Certificate(
            "Cette action ne concerne que les URL en https://.".to_string(),
        ));
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| TauriumError::Certificate("L'URL ne contient pas d'hôte.".to_string()))?
        .to_string();
    let port = parsed.port_or_known_default().unwrap_or(443);
    Ok((host, port))
}

/// Connects to `host:port` over TLS without validating the certificate and
/// returns the leaf certificate (DER) along with its SHA-256 fingerprint.
fn fetch_leaf_certificate(host: &str, port: u16) -> Result<(Vec<u8>, String), TauriumError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(|e| TauriumError::Certificate(e.to_string()))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AcceptAnyCert { provider }))
        .with_no_client_auth();

    let server_name = ServerName::try_from(host.to_string())
        .map_err(|_| TauriumError::Certificate(format!("Nom d'hôte invalide : {host}")))?;
    let mut conn = ClientConnection::new(Arc::new(config), server_name)
        .map_err(|e| TauriumError::Certificate(e.to_string()))?;

    // `TcpStream::connect` has no timeout of its own (it can hang well past
    // 10s against an unreachable host, the exact case this feature targets),
    // so resolve first and connect with an explicit deadline.
    let addr = (host, port)
        .to_socket_addrs()
        .map_err(TauriumError::Io)?
        .next()
        .ok_or_else(|| TauriumError::Certificate(format!("Impossible de résoudre {host}")))?;
    let mut sock =
        TcpStream::connect_timeout(&addr, Duration::from_secs(10)).map_err(TauriumError::Io)?;
    sock.set_read_timeout(Some(Duration::from_secs(10))).ok();
    sock.set_write_timeout(Some(Duration::from_secs(10))).ok();

    conn.complete_io(&mut sock).map_err(|e| {
        TauriumError::Certificate(format!("Échec de la connexion TLS à {host}:{port} : {e}"))
    })?;

    let certs = conn.peer_certificates().ok_or_else(|| {
        TauriumError::Certificate("Aucun certificat reçu du serveur.".to_string())
    })?;
    let leaf = certs
        .first()
        .ok_or_else(|| TauriumError::Certificate("Chaîne de certificats vide.".to_string()))?;

    let mut hasher = Sha256::new();
    hasher.update(leaf.as_ref());
    let digest = hasher.finalize();
    let fingerprint = digest
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":");

    Ok((leaf.as_ref().to_vec(), fingerprint))
}

/// Fetches the certificate presented by the service's URL, for display and
/// user confirmation (nothing is trusted yet at this point).
pub fn fetch_certificate_info(url: &str) -> Result<CertInfo, TauriumError> {
    let (host, port) = parse_host_port(url)?;
    let (_der, fingerprint) = fetch_leaf_certificate(&host, port)?;
    Ok(CertInfo {
        host,
        port,
        fingerprint,
    })
}

#[cfg(target_os = "windows")]
fn install_certificate(der: &[u8]) -> Result<(), TauriumError> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path =
        std::env::temp_dir().join(format!("taurium-cert-{}-{}.cer", std::process::id(), nanos));
    std::fs::write(&path, der)?;

    // `-user` targets the current user's trust store (Cert:\CurrentUser\Root),
    // which does not require an admin/UAC elevation.
    let result = std::process::Command::new("certutil")
        .args(["-user", "-addstore", "Root"])
        .arg(&path)
        .output();

    let _ = std::fs::remove_file(&path);

    let output = result
        .map_err(|e| TauriumError::Certificate(format!("Impossible de lancer certutil : {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(TauriumError::Certificate(format!(
            "certutil a échoué : {stderr}{stdout}"
        )));
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn install_certificate(_der: &[u8]) -> Result<(), TauriumError> {
    Err(TauriumError::Certificate(
        "L'installation automatique du certificat n'est prise en charge que sur Windows pour \
         le moment. Importez-le manuellement dans le magasin de certificats de confiance de \
         votre système."
            .to_string(),
    ))
}

/// Re-fetches the certificate at `host:port` and, only if it matches the
/// fingerprint the user already confirmed, installs it into the system trust
/// store. Re-checking here guards against the certificate having changed
/// between the confirmation step and this call.
pub fn trust_certificate(
    host: &str,
    port: u16,
    expected_fingerprint: &str,
) -> Result<(), TauriumError> {
    let (der, fingerprint) = fetch_leaf_certificate(host, port)?;
    if !fingerprint.eq_ignore_ascii_case(expected_fingerprint) {
        return Err(TauriumError::Certificate(
            "Le certificat présenté a changé depuis la vérification ; opération annulée par \
             sécurité."
                .to_string(),
        ));
    }
    install_certificate(&der)
}

#[cfg(test)]
mod tests {
    use super::parse_host_port;

    #[test]
    fn parse_host_port_defaults_to_443() {
        let (host, port) = parse_host_port("https://192.168.30.51/chat/").unwrap();
        assert_eq!(host, "192.168.30.51");
        assert_eq!(port, 443);
    }

    #[test]
    fn parse_host_port_honors_explicit_port() {
        let (host, port) = parse_host_port("https://nas.example.com:5001/chat/").unwrap();
        assert_eq!(host, "nas.example.com");
        assert_eq!(port, 5001);
    }

    #[test]
    fn parse_host_port_rejects_non_https() {
        assert!(parse_host_port("http://192.168.30.51/chat/").is_err());
    }

    #[test]
    fn parse_host_port_rejects_invalid_url() {
        assert!(parse_host_port("not a url").is_err());
    }
}
