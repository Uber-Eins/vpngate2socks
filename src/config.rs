//! Environment-driven application configuration.

use std::{
    env,
    net::{IpAddr, SocketAddr},
    num::NonZeroU16,
    path::{Component, Path, PathBuf},
    time::Duration,
};

use thiserror::Error;

use crate::domain::{SecretString, UpstreamEndpoint};

const DEFAULT_VPNGATE_URL: &str = "https://www.vpngate.net/api/iphone/";
const DEFAULT_IPPURE_URL: &str = "https://my.ippure.com/v1/info";

/// Username and password used by a local listener in explicit LAN mode.
#[derive(Clone)]
pub struct Credentials {
    pub username: String,
    pub password: SecretString,
}

/// PEM files used by the optional built-in HTTPS listener.
#[derive(Clone, Debug)]
pub struct TlsConfig {
    pub certificate: PathBuf,
    pub private_key: PathBuf,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Credentials")
            .field("username", &self.username)
            .field("password", &"[redacted]")
            .finish()
    }
}

/// Complete configuration for the control plane and privileged helper.
#[derive(Clone, Debug)]
pub struct AppConfig {
    pub web_bind: SocketAddr,
    pub socks_bind: SocketAddr,
    pub database_url: String,
    pub runtime_dir: PathBuf,
    pub netd_socket: PathBuf,
    pub web_dist_dir: PathBuf,
    pub vpngate_url: url::Url,
    pub ippure_url: url::Url,
    pub upstream: UpstreamEndpoint,
    pub refresh_interval: Duration,
    pub connect_timeout: Duration,
    pub ippure_timeout: Duration,
    pub max_parallel_tests: usize,
    pub lan_mode: bool,
    pub container_bind: bool,
    pub web_credentials: Option<Credentials>,
    pub socks_credentials: Option<Credentials>,
    pub tls: Option<TlsConfig>,
    pub unprivileged_uid: u32,
    pub unprivileged_gid: u32,
    pub openvpn_uid: u32,
}

/// Configuration error that can be shown safely at startup.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("missing required environment variable {0}")]
    Missing(&'static str),
    #[error("invalid {name}: {message}")]
    Invalid { name: &'static str, message: String },
    #[error("LAN mode requires {0}")]
    LanCredential(&'static str),
}

impl AppConfig {
    /// Parses configuration from `VPNGATE2SOCKS_*` environment variables.
    pub fn from_env() -> Result<Self, ConfigError> {
        let web_bind: SocketAddr = parse_or("VPNGATE2SOCKS_WEB_BIND", "127.0.0.1:8080")?;
        let socks_bind: SocketAddr = parse_or("VPNGATE2SOCKS_SOCKS_BIND", "127.0.0.1:1080")?;
        let lan_mode = parse_bool("VPNGATE2SOCKS_LAN_MODE", false)?;
        let container_bind = parse_bool("VPNGATE2SOCKS_CONTAINER_BIND", false)?;

        if !lan_mode
            && !container_bind
            && (!web_bind.ip().is_loopback() || !socks_bind.ip().is_loopback())
        {
            return Err(ConfigError::Invalid {
                name: "listener address",
                message: "non-loopback listeners require VPNGATE2SOCKS_LAN_MODE=true".to_owned(),
            });
        }
        if container_bind && (!web_bind.ip().is_unspecified() || !socks_bind.ip().is_unspecified())
        {
            return Err(ConfigError::Invalid {
                name: "container listener address",
                message: "container mode requires both listeners to use an unspecified address"
                    .to_owned(),
            });
        }

        let upstream_address = required("VPNGATE2SOCKS_UPSTREAM")?;
        let upstream_address =
            upstream_address
                .parse::<SocketAddr>()
                .map_err(|error| ConfigError::Invalid {
                    name: "VPNGATE2SOCKS_UPSTREAM",
                    message: error.to_string(),
                })?;
        let IpAddr::V4(upstream_host) = upstream_address.ip() else {
            return Err(ConfigError::Invalid {
                name: "VPNGATE2SOCKS_UPSTREAM",
                message: "only IPv4 upstream endpoints are supported".to_owned(),
            });
        };
        let upstream_port =
            NonZeroU16::new(upstream_address.port()).ok_or_else(|| ConfigError::Invalid {
                name: "VPNGATE2SOCKS_UPSTREAM",
                message: "port must be non-zero".to_owned(),
            })?;
        let upstream_user = config_value("VPNGATE2SOCKS_UPSTREAM_USER")?;
        let upstream_password =
            config_value("VPNGATE2SOCKS_UPSTREAM_PASSWORD")?.map(SecretString::new);
        let upstream = UpstreamEndpoint::new(
            upstream_host,
            upstream_port,
            upstream_user,
            upstream_password,
        )
        .map_err(|message| ConfigError::Invalid {
            name: "upstream credentials",
            message: message.to_owned(),
        })?;

        let runtime_dir = PathBuf::from(env_or("VPNGATE2SOCKS_RUNTIME_DIR", "/run/vpngate2socks"));
        validate_runtime_dir(&runtime_dir)?;
        let web_credentials = credentials_from_env(
            lan_mode,
            "VPNGATE2SOCKS_WEB_USER",
            "VPNGATE2SOCKS_WEB_PASSWORD",
        )?;
        let socks_credentials = credentials_from_env(
            lan_mode,
            "VPNGATE2SOCKS_SOCKS_USER",
            "VPNGATE2SOCKS_SOCKS_PASSWORD",
        )?;
        let tls_cert = env::var("VPNGATE2SOCKS_TLS_CERT").ok().map(PathBuf::from);
        let tls_key = env::var("VPNGATE2SOCKS_TLS_KEY").ok().map(PathBuf::from);
        if tls_cert.is_some() != tls_key.is_some() {
            return Err(ConfigError::Invalid {
                name: "TLS configuration",
                message: "certificate and key must be configured together".to_owned(),
            });
        }

        let tls = tls_cert
            .zip(tls_key)
            .map(|(certificate, private_key)| TlsConfig {
                certificate,
                private_key,
            });
        let unprivileged_uid = u32::try_from(parse_u64(
            "VPNGATE2SOCKS_UNPRIVILEGED_UID",
            10_001,
            1,
            u64::from(u32::MAX),
        )?)
        .map_err(|error| ConfigError::Invalid {
            name: "VPNGATE2SOCKS_UNPRIVILEGED_UID",
            message: error.to_string(),
        })?;
        let unprivileged_gid = u32::try_from(parse_u64(
            "VPNGATE2SOCKS_UNPRIVILEGED_GID",
            10_001,
            1,
            u64::from(u32::MAX),
        )?)
        .map_err(|error| ConfigError::Invalid {
            name: "VPNGATE2SOCKS_UNPRIVILEGED_GID",
            message: error.to_string(),
        })?;
        let openvpn_uid = u32::try_from(parse_u64(
            "VPNGATE2SOCKS_OPENVPN_UID",
            10_002,
            1,
            u64::from(u32::MAX),
        )?)
        .map_err(|error| ConfigError::Invalid {
            name: "VPNGATE2SOCKS_OPENVPN_UID",
            message: error.to_string(),
        })?;
        if openvpn_uid == unprivileged_uid {
            return Err(ConfigError::Invalid {
                name: "VPNGATE2SOCKS_OPENVPN_UID",
                message: "OpenVPN and the control plane must use different UIDs".to_owned(),
            });
        }

        Ok(Self {
            web_bind,
            socks_bind,
            database_url: env_or(
                "VPNGATE2SOCKS_DATABASE_URL",
                "sqlite:///var/lib/vpngate2socks/state.db?mode=rwc",
            ),
            netd_socket: runtime_dir.join("netd.sock"),
            runtime_dir,
            web_dist_dir: PathBuf::from(env_or("VPNGATE2SOCKS_WEB_DIST", "/opt/vpngate2socks/web")),
            vpngate_url: parse_url("VPNGATE2SOCKS_VPNGATE_URL", DEFAULT_VPNGATE_URL)?,
            ippure_url: parse_url("VPNGATE2SOCKS_IPPURE_URL", DEFAULT_IPPURE_URL)?,
            upstream,
            refresh_interval: Duration::from_secs(parse_u64(
                "VPNGATE2SOCKS_REFRESH_SECONDS",
                600,
                30,
                86_400,
            )?),
            connect_timeout: Duration::from_secs(parse_u64(
                "VPNGATE2SOCKS_CONNECT_TIMEOUT_SECONDS",
                45,
                5,
                300,
            )?),
            ippure_timeout: Duration::from_secs(parse_u64(
                "VPNGATE2SOCKS_IPPURE_TIMEOUT_SECONDS",
                15,
                1,
                120,
            )?),
            max_parallel_tests: usize::try_from(parse_u64(
                "VPNGATE2SOCKS_MAX_PARALLEL_TESTS",
                3,
                1,
                32,
            )?)
            .map_err(|error| ConfigError::Invalid {
                name: "VPNGATE2SOCKS_MAX_PARALLEL_TESTS",
                message: error.to_string(),
            })?,
            lan_mode,
            container_bind,
            web_credentials,
            socks_credentials,
            tls,
            unprivileged_uid,
            unprivileged_gid,
            openvpn_uid,
        })
    }
}

fn required(name: &'static str) -> Result<String, ConfigError> {
    env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or(ConfigError::Missing(name))
}

fn env_or(name: &'static str, default: &'static str) -> String {
    env::var(name).unwrap_or_else(|_| default.to_owned())
}

fn parse_or<T>(name: &'static str, default: &'static str) -> Result<T, ConfigError>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    env_or(name, default)
        .parse()
        .map_err(|error: T::Err| ConfigError::Invalid {
            name,
            message: error.to_string(),
        })
}

fn parse_url(name: &'static str, default: &'static str) -> Result<url::Url, ConfigError> {
    let url = url::Url::parse(&env_or(name, default)).map_err(|error| ConfigError::Invalid {
        name,
        message: error.to_string(),
    })?;
    if url.scheme() != "https" {
        return Err(ConfigError::Invalid {
            name,
            message: "URL must use HTTPS".to_owned(),
        });
    }
    Ok(url)
}

fn validate_runtime_dir(path: &Path) -> Result<(), ConfigError> {
    let Some(value) = path.to_str() else {
        return Err(ConfigError::Invalid {
            name: "VPNGATE2SOCKS_RUNTIME_DIR",
            message: "path must be valid UTF-8".to_owned(),
        });
    };
    let normal_components = path
        .components()
        .filter(|component| matches!(component, Component::Normal(_)))
        .count();
    let safe_components = path
        .components()
        .all(|component| matches!(component, Component::RootDir | Component::Normal(_)));
    if !path.is_absolute()
        || normal_components < 2
        || !safe_components
        || value.len() > 48
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-'))
    {
        return Err(ConfigError::Invalid {
            name: "VPNGATE2SOCKS_RUNTIME_DIR",
            message: "path must be an absolute, non-system root path without traversal, at most 48 bytes, and contain only safe characters".to_owned(),
        });
    }
    Ok(())
}

fn parse_bool(name: &'static str, default: bool) -> Result<bool, ConfigError> {
    match env::var(name) {
        Ok(value) => value.parse().map_err(|error| ConfigError::Invalid {
            name,
            message: format!("{error}"),
        }),
        Err(_) => Ok(default),
    }
}

fn parse_u64(
    name: &'static str,
    default: u64,
    minimum: u64,
    maximum: u64,
) -> Result<u64, ConfigError> {
    let value = match env::var(name) {
        Ok(value) => value.parse().map_err(|error| ConfigError::Invalid {
            name,
            message: format!("{error}"),
        })?,
        Err(_) => default,
    };
    if !(minimum..=maximum).contains(&value) {
        return Err(ConfigError::Invalid {
            name,
            message: format!("must be between {minimum} and {maximum}"),
        });
    }
    Ok(value)
}

fn credentials_from_env(
    required_in_lan_mode: bool,
    user_name: &'static str,
    password_name: &'static str,
) -> Result<Option<Credentials>, ConfigError> {
    let username = config_value(user_name)?;
    let password = config_value(password_name)?;
    match (username, password) {
        (Some(username), Some(password))
            if valid_listener_credential(&username) && valid_listener_credential(&password) =>
        {
            Ok(Some(Credentials {
                username,
                password: SecretString::new(password),
            }))
        }
        (None, None) if !required_in_lan_mode => Ok(None),
        (None, None) => Err(ConfigError::LanCredential(user_name)),
        _ => Err(ConfigError::Invalid {
            name: user_name,
            message: "username and password must be 1 to 255 bytes without NUL or line breaks"
                .to_owned(),
        }),
    }
}

fn valid_listener_credential(value: &str) -> bool {
    !value.is_empty()
        && u8::try_from(value.len()).is_ok()
        && !value
            .bytes()
            .any(|byte| matches!(byte, b'\0' | b'\r' | b'\n'))
}

fn config_value(name: &'static str) -> Result<Option<String>, ConfigError> {
    let direct = env::var(name).ok();
    let file_variable = format!("{name}_FILE");
    let file = env::var(&file_variable).ok();
    match (direct, file) {
        (Some(_), Some(_)) => Err(ConfigError::Invalid {
            name,
            message: format!("configure either {name} or {file_variable}, not both"),
        }),
        (Some(value), None) => Ok((!value.is_empty()).then_some(value)),
        (None, Some(path)) => {
            let value = read_secret_file(name, Path::new(&path))?;
            Ok((!value.is_empty()).then_some(value))
        }
        (None, None) => Ok(None),
    }
}

fn read_secret_file(name: &'static str, path: &Path) -> Result<String, ConfigError> {
    let metadata = std::fs::metadata(path).map_err(|error| ConfigError::Invalid {
        name,
        message: format!("cannot inspect secret file: {error}"),
    })?;
    if metadata.len() > 16 * 1024 {
        return Err(ConfigError::Invalid {
            name,
            message: "secret file exceeds 16 KiB".to_owned(),
        });
    }
    let value = std::fs::read_to_string(path).map_err(|error| ConfigError::Invalid {
        name,
        message: format!("cannot read secret file: {error}"),
    })?;
    Ok(value.trim_end_matches(['\r', '\n']).to_owned())
}

#[cfg(test)]
impl AppConfig {
    pub(crate) fn test_config(runtime_dir: PathBuf) -> Self {
        let upstream = UpstreamEndpoint::new(
            std::net::Ipv4Addr::LOCALHOST,
            NonZeroU16::new(9).expect("test port is non-zero"),
            None,
            None,
        )
        .expect("test upstream is valid");
        Self {
            web_bind: "127.0.0.1:0".parse().expect("test address is valid"),
            socks_bind: "127.0.0.1:0".parse().expect("test address is valid"),
            database_url: "sqlite::memory:".to_owned(),
            netd_socket: runtime_dir.join("netd.sock"),
            runtime_dir: runtime_dir.clone(),
            web_dist_dir: runtime_dir,
            vpngate_url: url::Url::parse(DEFAULT_VPNGATE_URL).expect("constant URL is valid"),
            ippure_url: url::Url::parse(DEFAULT_IPPURE_URL).expect("constant URL is valid"),
            upstream,
            refresh_interval: Duration::from_secs(600),
            connect_timeout: Duration::from_secs(1),
            ippure_timeout: Duration::from_secs(1),
            max_parallel_tests: 3,
            lan_mode: false,
            container_bind: false,
            web_credentials: None,
            socks_credentials: None,
            tls: None,
            unprivileged_uid: 10_001,
            unprivileged_gid: 10_001,
            openvpn_uid: 10_002,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_directory_rejects_traversal_and_broad_system_paths() {
        assert!(validate_runtime_dir(Path::new("/run/vpngate2socks")).is_ok());
        assert!(validate_runtime_dir(Path::new("/tmp/vpngate2socks")).is_ok());
        assert!(validate_runtime_dir(Path::new("/etc")).is_err());
        assert!(validate_runtime_dir(Path::new("/run/../etc/vpngate2socks")).is_err());
        assert!(validate_runtime_dir(Path::new("relative/runtime")).is_err());
    }

    #[test]
    fn listener_credentials_fit_the_socks_wire_format() {
        assert!(valid_listener_credential("user"));
        assert!(!valid_listener_credential(""));
        assert!(!valid_listener_credential(&"x".repeat(256)));
        assert!(!valid_listener_credential("line\nbreak"));
    }
}
