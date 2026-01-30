/*!
*notation-image-verifier* verifies container image signatures using the notation CLI.

Containerd invokes: `notation-image-verifier -name <ref> -digest <sha256:...>`

If no trust policy is configured, all images are allowed.
*/

use image_verifiers::{args, policy, reference};
use log::{error, info};
use serde::Deserialize;
use simplelog::{Config as LogConfig, LevelFilter, WriteLogger};
use std::{
    io::{self, Read},
    path::Path,
    process::{self, Command},
};

const NOTATION_TRUST_POLICY: &str = "/etc/containerd/image-verifiers/notation/trustpolicy.json";

/// Notation trust policy deserialized from trustpolicy.json.
#[derive(Deserialize)]
struct TrustPolicy {
    #[serde(rename = "trustPolicies")]
    trust_policies: Vec<serde_json::Value>,
}

impl policy::Policy for TrustPolicy {
    fn is_empty(&self) -> bool {
        self.trust_policies.is_empty()
    }
}

/// Loads the trust policy from the configured path.
fn load_trust_policy() -> policy::Result<Option<TrustPolicy>> {
    policy::load(Path::new(NOTATION_TRUST_POLICY))
}

fn main() {
    WriteLogger::init(LevelFilter::Info, LogConfig::default(), io::stdout()).ok();
    let args: args::Args = args::parse_go_style_args();
    let image_ref = reference::construct(&args.name, &args.digest);

    info!("verifying image: {}", image_ref);

    match load_trust_policy() {
        Ok(Some(_)) => {}
        Ok(None) => {
            info!("image verification skipped: no trust policy configured");
            return;
        }
        Err(e) => {
            error!("image verification failed: {}", e);
            process::exit(1);
        }
    }

    let output = match Command::new("notation")
        .args(["verify", &image_ref])
        .env(
            "NOTATION_CONFIG",
            "/etc/containerd/image-verifiers/notation",
        )
        .env("NOTATION_CACHE", "/var/cache/notation")
        .env("NOTATION_LIBEXEC", "/usr/libexec/notation-plugins")
        .env("HOME", "/root")
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            error!("image verification failed: {}", e);
            process::exit(1);
        }
    };

    if !output.status.success() {
        let msg = String::from_utf8_lossy(&output.stdout);
        let err = String::from_utf8_lossy(&output.stderr);
        error!("image verification failed: {}{}", msg, err);
        process::exit(1);
    }

    info!("image verification successful");

    // Drain stdin to avoid "file already closed" warnings from containerd.
    let _ = io::stdin().read_to_end(&mut Vec::new());
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_case::test_case;

    #[test]
    fn parse_trust_policy_with_policies() {
        let json = r#"{"trustPolicies": [{"name": "test"}]}"#;
        let policy: TrustPolicy = serde_json::from_str(json).unwrap();
        assert_eq!(policy.trust_policies.len(), 1);
    }

    #[test_case(r#"{"trustPolicies": []}"#, true; "empty array parses")]
    #[test_case(r#"{"trustPolicies": [{}]}"#, false; "non-empty array")]
    fn test_trust_policies_empty(json: &str, expected_empty: bool) {
        let policy: TrustPolicy = serde_json::from_str(json).unwrap();
        assert_eq!(policy.trust_policies.is_empty(), expected_empty);
    }
}
