use anyhow::Result;
use reqwest::Url;

pub(crate) fn server_url(address: &str, allow_http: bool) -> Result<Url> {
    let address = address.trim();
    let url = Url::parse(address)?;
    anyhow::ensure!(
        matches!(url.scheme(), "http" | "https")
            && address
                .split_once("://")
                .is_some_and(|(scheme, _)| scheme.eq_ignore_ascii_case(url.scheme())),
        "Use an HTTP or HTTPS sync server URL"
    );
    anyhow::ensure!(
        url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "Sync server URL cannot include credentials, a query or a fragment"
    );
    anyhow::ensure!(
        url.scheme() == "https"
            || allow_http
            || matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")),
        "Use HTTPS for remote sync servers or enable Allow HTTP"
    );
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Config;

    #[test]
    fn allow_http_preserves_the_scheme_and_requires_an_explicit_remote_http_url() {
        let https = "https://sync.example:8787/prefix";
        let http = "http://sync.example:8787/prefix";
        for allow_http in [false, true] {
            assert_eq!(server_url(https, allow_http).unwrap().as_str(), https);
        }
        assert!(server_url(http, false).is_err());
        assert_eq!(server_url(http, true).unwrap().as_str(), http);
        assert!(server_url("http:sync.example", true).is_err());
    }

    #[test]
    fn old_config_still_accepts_loopback_http() {
        let mut json = serde_json::to_value(Config::default()).unwrap();
        json.as_object_mut().unwrap().remove("allowHttp");
        let config: Config = serde_json::from_value(json).unwrap();
        assert!(!config.allow_http);
        for host in ["localhost", "127.0.0.1", "[::1]"] {
            assert!(server_url(&format!("http://{host}:8787"), config.allow_http).is_ok());
        }
    }

    #[test]
    fn sync_url_rejects_unsupported_components_even_when_http_is_allowed() {
        for address in [
            "ftp://sync.example",
            "http://user:secret@sync.example",
            "https://sync.example/path?query",
            "https://sync.example/path#fragment",
        ] {
            assert!(server_url(address, true).is_err(), "{address}");
        }
    }
}
