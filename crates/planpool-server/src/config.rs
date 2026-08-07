use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

// Explicit `name` attrs rather than `prefix = "PLANPOOL"`: confroid joins a
// container prefix with `__`, which would rename every documented variable
// (PLANPOOL_TOKEN -> PLANPOOL__TOKEN).
#[derive(confroid::Config)]
pub struct Config {
    /// Bearer token required for uploads and deletes; min 16 chars, generate with `openssl rand -hex 32`
    #[confroid(name = "PLANPOOL_TOKEN", example = "f3a9…64-hex-chars…c1")]
    pub token: String,
    /// Listen address
    #[confroid(name = "PLANPOOL_ADDR", default = SocketAddr::from(([0, 0, 0, 0], 8080)))]
    pub addr: SocketAddr,
    /// Directory where plan files are stored
    #[confroid(name = "PLANPOOL_DATA_DIR", default = "./plans")]
    pub data_dir: PathBuf,
    /// TTL applied when the upload doesn't specify one; humantime format, e.g. "12h", "7days"
    #[confroid(
        name = "PLANPOOL_DEFAULT_TTL",
        humantime,
        default = humantime::Duration::from(Duration::from_secs(7 * 24 * 60 * 60))
    )]
    pub default_ttl: Duration,
    /// Requested TTLs are clamped to this; humantime format
    #[confroid(
        name = "PLANPOOL_MAX_TTL",
        humantime,
        default = humantime::Duration::from(Duration::from_secs(30 * 24 * 60 * 60))
    )]
    pub max_ttl: Duration,
    /// Upload size limit; plain bytes or human-readable sizes like "5MB", "512KiB"
    #[confroid(
        name = "PLANPOOL_MAX_BODY_BYTES",
        bytesize,
        default = 5_242_880,
        example = "5MB"
    )]
    pub max_body_bytes: u64,
    /// Base URL used in returned plan links; falls back to the request's Host header when unset
    #[confroid(name = "PLANPOOL_PUBLIC_URL", example = "https://plans.example.com")]
    pub public_url: Option<String>,
}

impl Config {
    pub fn load() -> Result<Self, String> {
        confroid::from_env()
            .map_err(|e| e.to_string())
            .and_then(Self::validate)
    }

    fn validate(mut config: Config) -> Result<Config, String> {
        if config.token.len() < 16 {
            return Err("PLANPOOL_TOKEN must be at least 16 characters".into());
        }
        if let Some(url) = &mut config.public_url {
            *url = url.trim_end_matches('/').to_string();
        }
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load_from(pairs: &[(&str, &str)]) -> Result<Config, String> {
        confroid::from_pairs(pairs.iter().copied())
            .map_err(|e| e.to_string())
            .and_then(Config::validate)
    }

    #[test]
    fn defaults_apply_when_only_token_is_set() {
        let config = load_from(&[("PLANPOOL_TOKEN", "a-long-enough-token")]).unwrap();
        assert_eq!(config.addr, SocketAddr::from(([0, 0, 0, 0], 8080)));
        assert_eq!(config.data_dir, PathBuf::from("./plans"));
        assert_eq!(config.default_ttl, Duration::from_secs(604_800));
        assert_eq!(config.max_ttl, Duration::from_secs(2_592_000));
        assert_eq!(config.max_body_bytes, 5_242_880);
        assert_eq!(config.public_url, None);
    }

    #[test]
    fn missing_or_short_token_is_rejected() {
        assert!(load_from(&[]).is_err());
        assert!(load_from(&[("PLANPOOL_TOKEN", "short")]).is_err());
    }

    #[test]
    fn overrides_and_public_url_trimming() {
        let config = load_from(&[
            ("PLANPOOL_TOKEN", "a-long-enough-token"),
            ("PLANPOOL_ADDR", "127.0.0.1:9999"),
            ("PLANPOOL_DEFAULT_TTL", "1h 30m"),
            ("PLANPOOL_PUBLIC_URL", "https://plans.example.com/"),
        ])
        .unwrap();
        assert_eq!(config.addr, SocketAddr::from(([127, 0, 0, 1], 9999)));
        assert_eq!(config.default_ttl, Duration::from_secs(90 * 60));
        assert_eq!(
            config.public_url.as_deref(),
            Some("https://plans.example.com")
        );
    }

    #[test]
    fn max_body_accepts_human_readable_sizes() {
        for (value, expected) in [
            ("10MB", 10_000_000),
            ("2MiB", 2 * 1024 * 1024),
            ("5242880", 5_242_880),
        ] {
            let config = load_from(&[
                ("PLANPOOL_TOKEN", "a-long-enough-token"),
                ("PLANPOOL_MAX_BODY_BYTES", value),
            ])
            .unwrap();
            assert_eq!(config.max_body_bytes, expected, "for input {value:?}");
        }
    }

    #[test]
    fn ttl_without_a_unit_is_rejected() {
        let result = load_from(&[
            ("PLANPOOL_TOKEN", "a-long-enough-token"),
            ("PLANPOOL_DEFAULT_TTL", "604800"),
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn env_example_documents_every_variable() {
        let example = confroid::env_example::<Config>();
        for var in [
            "PLANPOOL_TOKEN=",
            "PLANPOOL_ADDR=0.0.0.0:8080",
            "PLANPOOL_DATA_DIR=./plans",
            "PLANPOOL_DEFAULT_TTL=7days",
            "PLANPOOL_MAX_TTL=30days",
            "PLANPOOL_MAX_BODY_BYTES=5242880",
            "PLANPOOL_PUBLIC_URL=",
        ] {
            assert!(example.contains(var), "missing `{var}` in:\n{example}");
        }
    }

    #[test]
    fn empty_public_url_reads_as_none() {
        let config = load_from(&[
            ("PLANPOOL_TOKEN", "a-long-enough-token"),
            ("PLANPOOL_PUBLIC_URL", ""),
        ])
        .unwrap();
        assert_eq!(config.public_url, None);
    }
}
