/*!
# Background

thar-be-registries generates containerd registry configuration from Bottlerocket settings.

It reads `/etc/containerd/thar-be-registries.toml` and writes per-registry configuration files to
`/etc/containerd/certs.d/`.

For each configured registry, it creates:
* `hosts.toml` - mirror endpoints with pull/resolve capabilities
* `credentials.toml` - authentication credentials (mode 0600)

## Behavior

* Exits successfully (0) if the input file doesn't exist (graceful no-op)
* Uses atomic directory replacement to avoid race conditions with containerd
* Containerd reads these files on-demand during image pulls (no restart needed)

*/

use log::{error, info, warn};
use nix::fcntl::{renameat2, RenameFlags};
use serde::Deserialize;
use simplelog::{Config as LogConfig, LevelFilter, SimpleLogger};
use snafu::ResultExt;
use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use url::Url;

mod creds;
mod error;
mod host_ns;
use creds::RegistryCredentials;
use error::{
    CreateDirSnafu, ParseSettingsSnafu, ReadSettingsSnafu, RenameDirSnafu, Result,
    SerializeTomlSnafu, WriteFileSnafu,
};
use host_ns::{Capability, Endpoint, HostConfig, HostNamespace};

const CERTS_DIR: &str = "/etc/containerd/certs.d";
const INPUT_FILE: &str = "/etc/containerd/thar-be-registries.toml";
const DOCKER_HUB_HOST: &str = "docker.io";
const DOCKER_HUB_REGISTRY: &str = "registry-1.docker.io";

/// Container registry settings
#[derive(Debug, Deserialize)]
struct Settings {
    mirrors: Option<Vec<Mirror>>,
    credentials: Option<Vec<Credential>>,
}

/// Registry mirror configuration
#[derive(Debug, Deserialize)]
struct Mirror {
    registry: String,
    endpoint: Vec<String>,
}

/// Registry authentication credentials
#[derive(Debug, Deserialize)]
struct Credential {
    registry: String,
    username: Option<String>,
    password: Option<String>,
    auth: Option<String>,
    identitytoken: Option<String>,
}

/// Entry point - generates containerd registry configuration files.
fn main() {
    if let Err(e) = run() {
        error!("{}", e);
        std::process::exit(1);
    }
}

/// Main execution logic
fn run() -> Result<()> {
    SimpleLogger::init(LevelFilter::Info, LogConfig::default()).unwrap_or(());
    info!("Reading registry config from {}", INPUT_FILE);

    if !Path::new(INPUT_FILE).exists() {
        warn!(
            "No registry settings file found at '{}', skipping",
            INPUT_FILE
        );
        return Ok(());
    }

    let settings = read_settings(INPUT_FILE)?;

    let temp_dir = tempfile::tempdir_in("/etc/containerd").context(CreateDirSnafu {
        path: "/etc/containerd",
    })?;

    let mirrors = settings.mirrors.unwrap_or_default();
    let credentials = settings.credentials.unwrap_or_default();
    let total_count = mirrors.len() + credentials.len();
    let mut failure_count = 0;

    for mirror in &mirrors {
        if let Err(e) = write_hosts_toml(temp_dir.path(), mirror) {
            error!(
                "Failed to write hosts.toml for '{}': {}",
                mirror.registry, e
            );
            failure_count += 1;
        }
    }

    for cred in &credentials {
        if let Err(e) = write_credentials_toml(temp_dir.path(), cred) {
            error!(
                "Failed to write credentials.toml for '{}': {}",
                cred.registry, e
            );
            failure_count += 1;
        }
    }

    if failure_count > 0 {
        return Err(error::Error::WriteRegistries {
            failure_count,
            total_count,
        });
    }

    // Ensure target exists for atomic swap.
    fs::create_dir_all(CERTS_DIR).context(CreateDirSnafu { path: CERTS_DIR })?;
    // After the exchange, the old content is removed when tmp_dir is dropped.
    rename_exchange_dir(temp_dir.path(), Path::new(CERTS_DIR))?;
    info!("Successfully wrote registry configs to {}", CERTS_DIR);

    Ok(())
}

/// Atomically exchange two directories using Linux renameat2.
/// Both paths must exist. After the call, each path points to what the other contained.
fn rename_exchange_dir(a: &Path, b: &Path) -> Result<()> {
    renameat2(None, a, None, b, RenameFlags::RENAME_EXCHANGE)
        .context(RenameDirSnafu { from: a, to: b })
}

/// Read and parse settings from TOML file
fn read_settings(path: &str) -> Result<Settings> {
    let contents = fs::read_to_string(path).context(ReadSettingsSnafu { path })?;
    toml::from_str(&contents).context(ParseSettingsSnafu { path })
}

/// Parse registry string, extracting host:port and optional scheme.
/// Works with full URLs (https://docker.io) or bare hostnames (docker.io, registry:5000).
/// Defaults to https scheme when none is provided.
fn parse_registry(registry: &str) -> (String, Option<String>) {
    // Try parsing as-is first (handles URLs with scheme like https://docker.io)
    // Only accept if it has a host (registry:5000 parses as scheme with no host)
    if let Ok(url) = Url::parse(registry) {
        if let Some(host) = url.host_str() {
            return (
                format_host_port(host, url.port()),
                Some(url.scheme().to_string()),
            );
        }
    }

    // Parse bare hostname with https:// prefix (handles docker.io or registry:5000)
    if let Ok(url) = Url::parse(&format!("https://{}", registry)) {
        if let Some(host) = url.host_str() {
            return (
                format_host_port(host, url.port()),
                Some("https".to_string()),
            );
        }
    }

    // Fallback: return as-is with https
    (registry.to_string(), Some("https".to_string()))
}

/// Encode registry name to directory name, replacing `:port` with `_port_`.
/// The trailing underscore matches containerd's hostDirectory() encoding,
/// which uses this format to avoid ambiguity with hostnames containing underscores.
fn encode_registry_name(name: &str) -> String {
    if let Some(idx) = name.rfind(':') {
        format!("{}_{}_", &name[..idx], &name[idx + 1..])
    } else {
        name.to_string()
    }
}

/// Format host with optional port
fn format_host_port(host: &str, port: Option<u16>) -> String {
    match port {
        Some(p) => format!("{}:{}", host, p),
        None => host.to_string(),
    }
}

/// Write hosts.toml for a registry mirror.
fn write_hosts_toml(base_dir: &Path, mirror: &Mirror) -> Result<()> {
    let (host, scheme) = parse_registry(&mirror.registry);
    let encoded = encode_registry_name(&host);
    let dir = base_dir.join(&encoded);
    fs::create_dir_all(&dir).context(CreateDirSnafu {
        path: dir.display().to_string(),
    })?;

    let scheme = scheme.as_deref().unwrap_or("https");
    let server = if host == DOCKER_HUB_HOST {
        format!("https://{}", DOCKER_HUB_REGISTRY)
    } else {
        format!("{}://{}", scheme, host)
    };

    let mut ns = HostNamespace {
        server: Some(server),
        ..Default::default()
    };

    for endpoint in &mirror.endpoint {
        let ep = Endpoint::new(endpoint);
        let mut cfg = HostConfig::new([Capability::Pull, Capability::Resolve]);
        if ep.has_path_component() {
            cfg = cfg.with_override_path(true);
        }
        ns.host.insert(ep, cfg);
    }

    let content = toml::to_string(&ns).context(SerializeTomlSnafu)?;
    let path = dir.join("hosts.toml");
    fs::write(&path, content).context(WriteFileSnafu {
        path: path.display().to_string(),
    })?;

    info!("Wrote hosts.toml for '{}'", mirror.registry);
    Ok(())
}

/// Write credentials.toml for a registry.
/// For docker.io, writes to registry-1.docker.io since containerd's credential
/// callback receives the actual registry host, not the image reference host.
fn write_credentials_toml(base_dir: &Path, cred: &Credential) -> Result<()> {
    let (host, _) = parse_registry(&cred.registry);
    let cred_host = if host == DOCKER_HUB_HOST {
        DOCKER_HUB_REGISTRY
    } else {
        &host
    };
    let encoded = encode_registry_name(cred_host);
    let dir = base_dir.join(&encoded);
    fs::create_dir_all(&dir).context(CreateDirSnafu {
        path: dir.display().to_string(),
    })?;

    let rc = RegistryCredentials {
        username: cred.username.clone(),
        password: cred.password.clone(),
        auth: cred.auth.clone(),
        identitytoken: cred.identitytoken.clone(),
    };

    let content = toml::to_string(&rc).context(SerializeTomlSnafu)?;
    let path = dir.join("credentials.toml");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)
        .context(WriteFileSnafu {
            path: path.display().to_string(),
        })?;
    file.write_all(content.as_bytes()).context(WriteFileSnafu {
        path: path.display().to_string(),
    })?;

    info!("Wrote credentials.toml for '{}'", cred.registry);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use error::Error;
    use test_case::test_case;

    // encode_registry_name tests
    #[test_case("docker.io", "docker.io"; "no port unchanged")]
    #[test_case("gcr.io", "gcr.io"; "gcr unchanged")]
    #[test_case("registry.example.com:5000", "registry.example.com_5000_"; "port encoded")]
    fn test_encode_registry_name(input: &str, expected: &str) {
        assert_eq!(encode_registry_name(input), expected);
    }

    // parse_registry tests
    #[test_case("docker.io", "docker.io", Some("https"); "bare hostname")]
    #[test_case("registry.example.com:5000", "registry.example.com:5000", Some("https"); "hostname with port")]
    #[test_case("https://docker.io", "docker.io", Some("https"); "https url")]
    #[test_case("http://registry.local:5000", "registry.local:5000", Some("http"); "http url with port")]
    fn test_parse_registry(input: &str, expected_host: &str, expected_scheme: Option<&str>) {
        let (host, scheme) = parse_registry(input);
        assert_eq!(host, expected_host);
        assert_eq!(scheme.as_deref(), expected_scheme);
    }

    // Settings schema tests
    #[test_case(r#"[[mirrors]]
 registry = "docker.io"
 endpoint = ["https://mirror.example.com"]"#, true, false; "mirrors only")]
    #[test_case(r#"[[credentials]]
 registry = "r.io"
 username = "u"
 password = "p""#, false, true; "credentials only")]
    fn test_settings_schema(toml_str: &str, has_mirrors: bool, has_creds: bool) {
        let toml_str = toml_str.replace(
            r"
", "
",
        );
        let settings: Settings = toml::from_str(&toml_str).unwrap();
        assert_eq!(settings.mirrors.is_some(), has_mirrors);
        assert_eq!(settings.credentials.is_some(), has_creds);
    }

    // Workflow tests - helper to create temp dir and verify file contents
    fn verify_file(dir: &std::path::Path, rel_path: &str, expected_contents: &[&str]) {
        let content = fs::read_to_string(dir.join(rel_path)).unwrap();
        for expected in expected_contents {
            assert!(
                content.contains(expected),
                "Missing '{}' in:
{}",
                expected,
                content
            );
        }
    }

    #[test_case(
    "docker.io",
    &["https://mirror.example.com"],
    "docker.io/hosts.toml",
    &[r#"server = "https://registry-1.docker.io""#, r#"[host."https://mirror.example.com"]"#]
    ; "docker.io mirror"
  )]
    #[test_case(
    "registry.example.com:5000",
    &["https://mirror.local"],
    "registry.example.com_5000_/hosts.toml",
    &[r#"server = "https://registry.example.com:5000""#]
    ; "registry with port"
  )]
    #[test_case(
    "docker.io",
    &["https://ecr-cache.example.com/v2/docker-hub"],
    "docker.io/hosts.toml",
    &["override_path = true"]
    ; "endpoint with path sets override_path"
  )]
    fn test_write_hosts_toml(
        registry: &str,
        endpoints: &[&str],
        expected_path: &str,
        expected_contents: &[&str],
    ) {
        let dir = tempfile::tempdir().unwrap();
        let mirror = Mirror {
            registry: registry.to_string(),
            endpoint: endpoints.iter().map(|s| s.to_string()).collect(),
        };
        write_hosts_toml(dir.path(), &mirror).unwrap();
        verify_file(dir.path(), expected_path, expected_contents);
    }

    #[test_case(
    "registry.example.com",
    Some("user"), Some("pass"), None, None,
    "registry.example.com/credentials.toml",
    &[r#"username = "user""#, r#"password = "pass""#]
    ; "username and password"
  )]
    #[test_case(
    "docker.io",
    Some("user"), Some("pass"), None, None,
    "registry-1.docker.io/credentials.toml",
    &[r#"username = "user""#, r#"password = "pass""#]
    ; "docker.io writes to registry-1.docker.io"
  )]
    #[test_case(
    "registry.example.com",
    None, None, None, Some("token123"),
    "registry.example.com/credentials.toml",
    &[r#"identitytoken = "token123""#]
    ; "identitytoken only"
  )]
    #[test_case(
    "registry.example.com",
    None, None, Some("dXNlcjpwYXNz"), None,
    "registry.example.com/credentials.toml",
    &[r#"auth = "dXNlcjpwYXNz""#]
    ; "auth only"
  )]
    fn test_write_credentials_toml(
        registry: &str,
        username: Option<&str>,
        password: Option<&str>,
        auth: Option<&str>,
        identitytoken: Option<&str>,
        expected_path: &str,
        expected_contents: &[&str],
    ) {
        let dir = tempfile::tempdir().unwrap();
        let cred = Credential {
            registry: registry.to_string(),
            username: username.map(String::from),
            password: password.map(String::from),
            auth: auth.map(String::from),
            identitytoken: identitytoken.map(String::from),
        };
        write_credentials_toml(dir.path(), &cred).unwrap();
        verify_file(dir.path(), expected_path, expected_contents);
    }

    #[test]
    fn test_workflow_mirror_and_credential_same_registry() {
        let dir = tempfile::tempdir().unwrap();
        let mirror = Mirror {
            registry: "registry.example.com".to_string(),
            endpoint: vec!["https://mirror.example.com".to_string()],
        };
        let cred = Credential {
            registry: "registry.example.com".to_string(),
            username: Some("user".to_string()),
            password: Some("pass".to_string()),
            auth: None,
            identitytoken: None,
        };
        write_hosts_toml(dir.path(), &mirror).unwrap();
        write_credentials_toml(dir.path(), &cred).unwrap();
        assert!(dir.path().join("registry.example.com/hosts.toml").exists());
        assert!(dir
            .path()
            .join("registry.example.com/credentials.toml")
            .exists());
    }

    #[test]
    fn test_credentials_file_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let cred = Credential {
            registry: "test.io".to_string(),
            username: Some("u".to_string()),
            password: Some("p".to_string()),
            auth: None,
            identitytoken: None,
        };
        write_credentials_toml(dir.path(), &cred).unwrap();
        let path = dir.path().join("test.io/credentials.toml");
        let perms = fs::metadata(&path).unwrap().permissions();
        assert_eq!(
            perms.mode() & 0o777,
            0o600,
            "credentials.toml should be mode 0600"
        );
    }

    #[test]
    fn test_read_settings_file_not_found() {
        let result = read_settings("/nonexistent/path/to/file.toml");
        assert!(matches!(result, Err(Error::ReadSettings { .. })));
    }

    #[test]
    fn test_read_settings_invalid_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("invalid.toml");
        fs::write(&path, "this is not valid toml [[[").unwrap();
        let result = read_settings(path.to_str().unwrap());
        assert!(matches!(result, Err(Error::ParseSettings { .. })));
    }

    #[test]
    fn test_docker_io_special_server() {
        let dir = tempfile::tempdir().unwrap();
        let mirror = Mirror {
            registry: "docker.io".to_string(),
            endpoint: vec!["https://mirror.example.com".to_string()],
        };
        write_hosts_toml(dir.path(), &mirror).unwrap();
        let content = fs::read_to_string(dir.path().join("docker.io/hosts.toml")).unwrap();
        assert!(content.contains(r#"server = "https://registry-1.docker.io""#));
    }

    #[test]
    fn test_multiple_endpoints() {
        let dir = tempfile::tempdir().unwrap();
        let mirror = Mirror {
            registry: "test.io".to_string(),
            endpoint: vec![
                "https://mirror1.example.com".to_string(),
                "https://mirror2.example.com".to_string(),
                "https://mirror3.example.com".to_string(),
            ],
        };
        write_hosts_toml(dir.path(), &mirror).unwrap();
        let content = fs::read_to_string(dir.path().join("test.io/hosts.toml")).unwrap();
        assert!(content.contains(r#"[host."https://mirror1.example.com"]"#));
        assert!(content.contains(r#"[host."https://mirror2.example.com"]"#));
        assert!(content.contains(r#"[host."https://mirror3.example.com"]"#));
        assert_eq!(content.matches("capabilities").count(), 3);
    }

    #[test]
    fn test_empty_endpoints() {
        let dir = tempfile::tempdir().unwrap();
        let mirror = Mirror {
            registry: "test.io".to_string(),
            endpoint: vec![],
        };
        write_hosts_toml(dir.path(), &mirror).unwrap();
        let content = fs::read_to_string(dir.path().join("test.io/hosts.toml")).unwrap();
        assert!(content.contains(r#"server = "https://test.io""#));
        assert!(!content.contains("[host."));
    }

    #[test]
    fn test_rename_exchange_dir() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        fs::create_dir(&a).unwrap();
        fs::create_dir(&b).unwrap();
        fs::write(a.join("file"), "from_a").unwrap();
        fs::write(b.join("file"), "from_b").unwrap();

        rename_exchange_dir(&a, &b).unwrap();

        assert_eq!(fs::read_to_string(a.join("file")).unwrap(), "from_b");
        assert_eq!(fs::read_to_string(b.join("file")).unwrap(), "from_a");
    }
}
