//! Mihomo proxy-provider rendering for sanitized VPN Gate profiles.

use crate::{domain::VpnNode, openvpn::SanitizedOpenVpn};

struct MihomoOpenVpn {
    ca: String,
    cert: String,
    key: String,
    tls_auth: Option<String>,
    tls_crypt: Option<String>,
    cipher: Option<String>,
    auth: Option<String>,
    data_ciphers: Vec<String>,
    data_ciphers_fallback: Option<String>,
    key_direction: Option<String>,
}

impl MihomoOpenVpn {
    fn from_sanitized(profile: &SanitizedOpenVpn) -> Option<Self> {
        let text = profile.as_str();
        let cipher = scalar(text, "cipher");
        let auth = scalar(text, "auth");
        let data_ciphers = scalar(text, "data-ciphers")
            .map(|value| {
                value
                    .split([':', ','])
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let data_ciphers_fallback = scalar(text, "data-ciphers-fallback");

        if cipher
            .as_deref()
            .is_some_and(|value| !is_mihomo_cipher(value))
            || data_ciphers.iter().any(|value| !is_mihomo_cipher(value))
            || data_ciphers_fallback
                .as_deref()
                .is_some_and(|value| !is_mihomo_cipher(value))
            || auth.as_deref().is_some_and(|value| !is_mihomo_auth(value))
        {
            return None;
        }

        let tls_auth = inline_block(text, "tls-auth");
        let tls_crypt = inline_block(text, "tls-crypt");
        if tls_auth.is_some() && tls_crypt.is_some() {
            return None;
        }

        Some(Self {
            ca: inline_block(text, "ca")?,
            cert: inline_block(text, "cert")?,
            key: inline_block(text, "key")?,
            tls_auth,
            tls_crypt,
            cipher,
            auth,
            data_ciphers,
            data_ciphers_fallback,
            key_direction: scalar(text, "key-direction"),
        })
    }
}

/// Renders every currently available, Mihomo-compatible profile.
pub(crate) fn render_provider(nodes: &[VpnNode]) -> String {
    let mut output = String::from("proxies:");
    let mut rendered = 0_usize;

    for node in nodes {
        let Some(openvpn) = node.openvpn.as_ref() else {
            continue;
        };
        let Some(profile) = MihomoOpenVpn::from_sanitized(openvpn) else {
            continue;
        };
        if rendered == 0 {
            output.push('\n');
        }
        rendered += 1;

        output.push_str("  - name: vpngate-");
        output.push_str(node.id.as_str());
        output.push_str("\n    type: openvpn\n    server: ");
        output.push_str(&openvpn.remote().ip().to_string());
        output.push_str("\n    port: ");
        output.push_str(&openvpn.remote().port().to_string());
        output.push_str("\n    proto: tcp\n    dev: tun\n");
        push_optional_scalar(&mut output, "cipher", profile.cipher.as_deref());
        push_optional_scalar(&mut output, "auth", profile.auth.as_deref());
        if !profile.data_ciphers.is_empty() {
            output.push_str("    data-ciphers:\n");
            for cipher in &profile.data_ciphers {
                output.push_str("      - ");
                output.push_str(cipher);
                output.push('\n');
            }
        }
        push_optional_scalar(
            &mut output,
            "data-ciphers-fallback",
            profile.data_ciphers_fallback.as_deref(),
        );
        if let Some(direction) = profile.key_direction.as_deref() {
            output.push_str("    key-direction: \"");
            output.push_str(direction);
            output.push_str("\"\n");
        }
        push_block(&mut output, "ca", &profile.ca);
        push_block(&mut output, "cert", &profile.cert);
        push_block(&mut output, "key", &profile.key);
        if let Some(value) = profile.tls_auth.as_deref() {
            push_block(&mut output, "tls-auth", value);
        }
        if let Some(value) = profile.tls_crypt.as_deref() {
            push_block(&mut output, "tls-crypt", value);
        }
        output.push_str("    remote-dns-resolve: true\n    dns: [1.1.1.1, 8.8.8.8]\n");
    }

    if rendered == 0 {
        output.push_str(" []\n");
    }
    output
}

fn scalar(profile: &str, directive: &str) -> Option<String> {
    profile.lines().find_map(|line| {
        let (name, value) = line.split_once(' ')?;
        (name == directive).then(|| value.to_owned())
    })
}

fn inline_block(profile: &str, name: &str) -> Option<String> {
    let opening = format!("<{name}>");
    let closing = format!("</{name}>");
    let mut lines = profile.lines();
    lines.find(|line| *line == opening)?;

    let mut body = String::new();
    for line in lines {
        if line == closing {
            return (!body.is_empty()).then_some(body);
        }
        body.push_str(line);
        body.push('\n');
    }
    None
}

fn is_mihomo_cipher(value: &str) -> bool {
    matches!(
        value.to_ascii_uppercase().as_str(),
        "AES-CBC"
            | "AES-128-CBC"
            | "AES-192-CBC"
            | "AES-256-CBC"
            | "AES-128-GCM"
            | "AES-192-GCM"
            | "AES-256-GCM"
            | "CHACHA20-POLY1305"
    )
}

fn is_mihomo_auth(value: &str) -> bool {
    matches!(
        value.to_ascii_uppercase().as_str(),
        "MD5" | "SHA1" | "SHA256" | "SHA384" | "SHA512"
    )
}

fn push_optional_scalar(output: &mut String, name: &str, value: Option<&str>) {
    if let Some(value) = value {
        output.push_str("    ");
        output.push_str(name);
        output.push_str(": ");
        output.push_str(value);
        output.push('\n');
    }
}

fn push_block(output: &mut String, name: &str, value: &str) {
    output.push_str("    ");
    output.push_str(name);
    output.push_str(": |\n");
    for line in value.lines() {
        output.push_str("      ");
        output.push_str(line);
        output.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use std::{net::Ipv4Addr, num::NonZeroU16};

    use crate::{
        domain::{NodeAvailability, NodeId, VpnNode},
        openvpn::sanitize_openvpn,
    };

    use super::*;

    #[test]
    fn renders_a_mihomo_openvpn_provider() {
        let ip = Ipv4Addr::new(1, 2, 3, 4);
        let profile = sanitize_openvpn(
            b"client
dev tun
proto tcp
remote 1.2.3.4 443
cipher AES-128-CBC
auth SHA1
data-ciphers AES-256-GCM:AES-128-GCM
data-ciphers-fallback AES-128-CBC
key-direction 1
<ca>
CA
</ca>
<cert>
CERT
</cert>
<key>
KEY
</key>
<tls-auth>
TLS-AUTH
</tls-auth>
",
            ip,
        )
        .expect("sanitized profile");
        let node_id = NodeId::from_digest(profile.digest());
        let node = VpnNode {
            id: node_id.clone(),
            hostname: "vpn.example".to_owned(),
            ip,
            score: 1,
            ping_ms: Some(10),
            speed_bps: 1,
            country_long: "Japan".to_owned(),
            country_short: "JP".to_owned(),
            sessions: 1,
            uptime_ms: 1,
            total_users: 1,
            total_traffic_bytes: 1,
            log_type: String::new(),
            operator: String::new(),
            message: String::new(),
            tcp_port: NonZeroU16::new(443),
            availability: NodeAvailability::Available,
            openvpn: Some(profile),
        };

        let output = render_provider(&[node]);
        assert!(output.starts_with(&format!("proxies:\n  - name: vpngate-{node_id}\n")));
        assert!(
            output.contains(
                "    type: openvpn\n    server: 1.2.3.4\n    port: 443\n    proto: tcp\n"
            )
        );
        assert!(output.contains("    data-ciphers:\n      - AES-256-GCM\n      - AES-128-GCM\n"));
        assert!(output.contains("    key-direction: \"1\"\n"));
        assert!(output.contains("    ca: |\n      CA\n"));
        assert!(output.contains("    tls-auth: |\n      TLS-AUTH\n"));
        assert!(output.contains("    remote-dns-resolve: true\n    dns: [1.1.1.1, 8.8.8.8]\n"));
    }
}
