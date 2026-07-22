//! Strict parser that converts untrusted VPN Gate profiles into a local allowlist.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    net::{Ipv4Addr, SocketAddrV4},
    num::NonZeroU16,
};

use thiserror::Error;
use zeroize::Zeroize as _;

const MAX_PROFILE_BYTES: usize = 256 * 1024;
const MAX_LINE_BYTES: usize = 4096;
const MAX_BLOCK_BYTES: usize = 96 * 1024;

/// A locally regenerated profile that contains no external file or script references.
#[derive(Clone)]
pub struct SanitizedOpenVpn {
    rendered: String,
    remote: SocketAddrV4,
    digest: blake3::Hash,
}

impl SanitizedOpenVpn {
    /// Returns the complete sanitized profile for the privileged helper.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.rendered
    }

    /// Returns the validated TCP VPN endpoint.
    #[must_use]
    pub const fn remote(&self) -> SocketAddrV4 {
        self.remote
    }

    /// Returns the digest used in stable node identity.
    #[must_use]
    pub const fn digest(&self) -> blake3::Hash {
        self.digest
    }
}

impl fmt::Debug for SanitizedOpenVpn {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SanitizedOpenVpn")
            .field("remote", &self.remote)
            .field("digest", &self.digest.to_hex().as_str())
            .finish_non_exhaustive()
    }
}

impl Drop for SanitizedOpenVpn {
    fn drop(&mut self) {
        self.rendered.zeroize();
    }
}

/// Why an untrusted `OpenVPN` profile was rejected.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum OpenVpnConfigError {
    #[error("profile exceeds the {MAX_PROFILE_BYTES}-byte limit")]
    TooLarge,
    #[error("profile is not valid UTF-8")]
    InvalidUtf8,
    #[error("profile line exceeds the {MAX_LINE_BYTES}-byte limit")]
    LineTooLong,
    #[error("unsupported OpenVPN directive: {0}")]
    UnsupportedDirective(String),
    #[error("dangerous OpenVPN directive: {0}")]
    DangerousDirective(String),
    #[error("OpenVPN profile references an external file: {0}")]
    ExternalFile(String),
    #[error("only TCP OpenVPN profiles are supported")]
    UnsupportedProtocol,
    #[error("OpenVPN profile must use dev tun")]
    InvalidDevice,
    #[error("OpenVPN profile has an invalid remote directive")]
    InvalidRemote,
    #[error("OpenVPN remote does not match the VPN Gate CSV address")]
    RemoteMismatch,
    #[error("missing required inline block: {0}")]
    MissingInlineBlock(&'static str),
    #[error("invalid or unterminated inline block: {0}")]
    InvalidInlineBlock(String),
    #[error("invalid value for OpenVPN directive: {0}")]
    InvalidValue(String),
}

/// Parses and regenerates a downloaded `OpenVPN` profile using a fixed directive allowlist.
pub fn sanitize_openvpn(
    input: &[u8],
    expected_ip: Ipv4Addr,
) -> Result<SanitizedOpenVpn, OpenVpnConfigError> {
    if input.len() > MAX_PROFILE_BYTES {
        return Err(OpenVpnConfigError::TooLarge);
    }
    let text = std::str::from_utf8(input).map_err(|_| OpenVpnConfigError::InvalidUtf8)?;
    if text.lines().any(|line| line.len() > MAX_LINE_BYTES) {
        return Err(OpenVpnConfigError::LineTooLong);
    }

    let mut blocks = BTreeMap::<String, String>::new();
    let mut options = BTreeSet::<String>::new();
    let mut remote = None;
    let mut saw_tcp = false;
    let mut saw_tun = false;
    let mut lines = text.lines();

    while let Some(raw_line) = lines.next() {
        let line = raw_line.trim().trim_end_matches('\r');
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }

        if let Some(block_name) = opening_block(line) {
            if !matches!(block_name, "ca" | "cert" | "key" | "tls-auth" | "tls-crypt") {
                return Err(OpenVpnConfigError::UnsupportedDirective(format!(
                    "<{block_name}>"
                )));
            }
            let close = format!("</{block_name}>");
            let mut body = String::new();
            let mut closed = false;
            for block_line in lines.by_ref() {
                let block_line = block_line.trim_end_matches('\r');
                if block_line.trim() == close {
                    closed = true;
                    break;
                }
                if opening_block(block_line.trim()).is_some() || block_line.as_bytes().contains(&0)
                {
                    return Err(OpenVpnConfigError::InvalidInlineBlock(
                        block_name.to_owned(),
                    ));
                }
                if body
                    .len()
                    .saturating_add(block_line.len())
                    .saturating_add(1)
                    > MAX_BLOCK_BYTES
                {
                    return Err(OpenVpnConfigError::InvalidInlineBlock(
                        block_name.to_owned(),
                    ));
                }
                body.push_str(block_line);
                body.push('\n');
            }
            if !closed
                || body.trim().is_empty()
                || blocks.insert(block_name.to_owned(), body).is_some()
            {
                return Err(OpenVpnConfigError::InvalidInlineBlock(
                    block_name.to_owned(),
                ));
            }
            continue;
        }

        let tokens: Vec<&str> = line.split_ascii_whitespace().collect();
        let Some(directive) = tokens.first().copied() else {
            continue;
        };
        let directive = directive.to_ascii_lowercase();

        if is_dangerous(&directive) {
            return Err(OpenVpnConfigError::DangerousDirective(directive));
        }

        match directive.as_str() {
            "proto" => {
                let protocol = exactly_one_value(&tokens, "proto")?.to_ascii_lowercase();
                if !matches!(
                    protocol.as_str(),
                    "tcp" | "tcp-client" | "tcp4" | "tcp4-client"
                ) {
                    return Err(OpenVpnConfigError::UnsupportedProtocol);
                }
                saw_tcp = true;
            }
            "dev" => {
                if exactly_one_value(&tokens, "dev")? != "tun" {
                    return Err(OpenVpnConfigError::InvalidDevice);
                }
                saw_tun = true;
            }
            "dev-type" => {
                if exactly_one_value(&tokens, "dev-type")? != "tun" {
                    return Err(OpenVpnConfigError::InvalidDevice);
                }
            }
            "remote" => {
                if remote.is_some() || !(3..=4).contains(&tokens.len()) {
                    return Err(OpenVpnConfigError::InvalidRemote);
                }
                let host = tokens[1]
                    .parse::<Ipv4Addr>()
                    .map_err(|_| OpenVpnConfigError::InvalidRemote)?;
                if host != expected_ip {
                    return Err(OpenVpnConfigError::RemoteMismatch);
                }
                let port = tokens[2]
                    .parse::<u16>()
                    .ok()
                    .and_then(NonZeroU16::new)
                    .ok_or(OpenVpnConfigError::InvalidRemote)?;
                if let Some(protocol) = tokens.get(3) {
                    if !matches!(
                        protocol.to_ascii_lowercase().as_str(),
                        "tcp" | "tcp-client" | "tcp4" | "tcp4-client"
                    ) {
                        return Err(OpenVpnConfigError::UnsupportedProtocol);
                    }
                }
                remote = Some(SocketAddrV4::new(host, port.get()));
            }
            "ca" | "cert" | "key" | "tls-auth" | "tls-crypt" | "pkcs12" => {
                return Err(OpenVpnConfigError::ExternalFile(directive));
            }
            "cipher"
            | "auth"
            | "data-ciphers"
            | "data-ciphers-fallback"
            | "tls-version-min"
            | "tls-cipher" => {
                let value = exactly_one_value(&tokens, &directive)?;
                if !is_safe_crypto_value(value) {
                    return Err(OpenVpnConfigError::InvalidValue(directive));
                }
                options.insert(format!("{directive} {value}"));
            }
            "key-direction" => {
                let value = exactly_one_value(&tokens, "key-direction")?;
                if !matches!(value, "0" | "1") {
                    return Err(OpenVpnConfigError::InvalidValue(directive));
                }
                options.insert(format!("key-direction {value}"));
            }
            "remote-cert-tls" => {
                if exactly_one_value(&tokens, "remote-cert-tls")? != "server" {
                    return Err(OpenVpnConfigError::InvalidValue(directive));
                }
                options.insert("remote-cert-tls server".to_owned());
            }
            "client" | "tls-client" | "nobind" | "persist-key" | "persist-tun" | "pull"
            | "auth-nocache" => ensure_no_values(&tokens, &directive)?,
            "resolv-retry"
            | "connect-retry"
            | "connect-retry-max"
            | "connect-timeout"
            | "server-poll-timeout"
            | "verb"
            | "mute" => {
                validate_ignored_scalar(&tokens, &directive)?;
            }
            "setenv" if tokens.as_slice() == ["setenv", "opt", "block-outside-dns"] => {}
            _ => return Err(OpenVpnConfigError::UnsupportedDirective(directive)),
        }
    }

    if !saw_tcp {
        return Err(OpenVpnConfigError::UnsupportedProtocol);
    }
    if !saw_tun {
        return Err(OpenVpnConfigError::InvalidDevice);
    }
    let remote = remote.ok_or(OpenVpnConfigError::InvalidRemote)?;
    for required in ["ca", "cert", "key"] {
        if !blocks.contains_key(required) {
            return Err(OpenVpnConfigError::MissingInlineBlock(required));
        }
    }

    let mut rendered = format!(
        "client\ndev tun\nproto tcp-client\nremote {} {} tcp-client\nnobind\npersist-key\npersist-tun\nauth-nocache\nverb 3\n",
        remote.ip(),
        remote.port()
    );
    if !options
        .iter()
        .any(|option| option == "remote-cert-tls server")
    {
        rendered.push_str("remote-cert-tls server\n");
    }
    for option in options {
        rendered.push_str(&option);
        rendered.push('\n');
    }
    for block_name in ["ca", "cert", "key", "tls-auth", "tls-crypt"] {
        if let Some(body) = blocks.get(block_name) {
            rendered.push('<');
            rendered.push_str(block_name);
            rendered.push_str(">\n");
            rendered.push_str(body);
            rendered.push_str("</");
            rendered.push_str(block_name);
            rendered.push_str(">\n");
        }
    }
    let digest = blake3::hash(rendered.as_bytes());

    Ok(SanitizedOpenVpn {
        rendered,
        remote,
        digest,
    })
}

fn opening_block(line: &str) -> Option<&str> {
    line.strip_prefix('<')
        .and_then(|value| value.strip_suffix('>'))
        .filter(|value| !value.starts_with('/') && !value.is_empty())
}

fn exactly_one_value<'a>(
    tokens: &'a [&str],
    directive: &str,
) -> Result<&'a str, OpenVpnConfigError> {
    match tokens {
        [_, value] => Ok(value),
        _ => Err(OpenVpnConfigError::InvalidValue(directive.to_owned())),
    }
}

fn ensure_no_values(tokens: &[&str], directive: &str) -> Result<(), OpenVpnConfigError> {
    if tokens.len() == 1 {
        Ok(())
    } else {
        Err(OpenVpnConfigError::InvalidValue(directive.to_owned()))
    }
}

fn validate_ignored_scalar(tokens: &[&str], directive: &str) -> Result<(), OpenVpnConfigError> {
    if tokens.len() == 2
        && tokens[1].len() <= 16
        && tokens[1]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        Ok(())
    } else {
        Err(OpenVpnConfigError::InvalidValue(directive.to_owned()))
    }
}

fn is_safe_crypto_value(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':' | b',' | b'.')
        })
}

fn is_dangerous(directive: &str) -> bool {
    matches!(
        directive,
        "up" | "down"
            | "route-up"
            | "route-pre-down"
            | "ipchange"
            | "client-connect"
            | "client-disconnect"
            | "learn-address"
            | "auth-user-pass-verify"
            | "tls-verify"
            | "plugin"
            | "script-security"
            | "management"
            | "management-client"
            | "management-external-key"
            | "management-external-cert"
            | "management-query-passwords"
            | "management-hold"
            | "daemon"
            | "log"
            | "log-append"
            | "status"
            | "writepid"
            | "chroot"
            | "cd"
            | "user"
            | "group"
            | "route"
            | "route-ipv6"
            | "redirect-gateway"
            | "ifconfig"
            | "ifconfig-ipv6"
            | "socks-proxy"
            | "http-proxy"
            | "auth-user-pass"
    )
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    const IP: Ipv4Addr = Ipv4Addr::new(1, 2, 3, 4);

    fn valid_profile() -> String {
        "client\ndev tun\nproto tcp\nremote 1.2.3.4 443\ncipher AES-128-CBC\nauth SHA1\n<ca>\nCA\n</ca>\n<cert>\nCERT\n</cert>\n<key>\nKEY\n</key>\n".to_owned()
    }

    #[test]
    fn sanitizes_tcp_profile_and_is_stable() {
        let first = sanitize_openvpn(valid_profile().as_bytes(), IP).expect("valid profile");
        let second = sanitize_openvpn(valid_profile().as_bytes(), IP).expect("valid profile");
        assert_eq!(first.remote(), SocketAddrV4::new(IP, 443));
        assert_eq!(first.digest(), second.digest());
        assert!(!first.as_str().contains("script-security"));
    }

    #[test]
    fn rejects_scripts_and_external_files() {
        let scripted = valid_profile().replace("client\n", "client\nup /tmp/pwn\n");
        assert!(matches!(
            sanitize_openvpn(scripted.as_bytes(), IP),
            Err(OpenVpnConfigError::DangerousDirective(_))
        ));

        let external = valid_profile().replace("<ca>\nCA\n</ca>", "ca /tmp/ca.pem");
        assert!(matches!(
            sanitize_openvpn(external.as_bytes(), IP),
            Err(OpenVpnConfigError::ExternalFile(_))
        ));
    }

    #[test]
    fn rejects_udp_profiles() {
        let udp = valid_profile().replace("proto tcp", "proto udp");
        assert!(matches!(
            sanitize_openvpn(udp.as_bytes(), IP),
            Err(OpenVpnConfigError::UnsupportedProtocol)
        ));
    }

    proptest! {
        #[test]
        fn arbitrary_profiles_never_panic(input in proptest::collection::vec(any::<u8>(), 0..4096)) {
            let _result = sanitize_openvpn(&input, IP);
        }
    }
}
