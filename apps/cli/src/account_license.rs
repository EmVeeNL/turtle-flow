//! Explicit account linking. The cached lease is offline-verifiable; local Free use never depends on it.

use std::{io::Write, process::Command, time::Duration};

use anyhow::{Context, Result, bail};
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use ed25519_dalek::{Signature, VerifyingKey};
use keyring::Entry;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

const PORTAL: &str = "https://dash.turtle-flow.com";
const KEY_ID: &str = "tf-free-2026-1";
const PUBLIC_KEY: &str = "/4WNdZD0l45Lh8VyMYJnrzgV5oNm0bW08Pc2AGGAZbA=";
const KEYRING_SERVICE: &str = "turtle-flow-cli";
const KEYRING_ACCOUNT: &str = "installation-signing-key";

#[derive(Debug, Serialize)]
struct AuthorizationRequest<'a> {
    public_key: String,
    device_name: &'a str,
    platform: &'a str,
    cli_version: &'a str,
}

#[derive(Debug, Deserialize)]
struct AuthorizationResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: u64,
}

#[derive(Debug, Deserialize, Serialize)]
struct LeaseEnvelope {
    payload: Value,
    key_id: String,
    signature: String,
}

fn key_entry() -> Result<Entry> {
    Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT).context("opening the OS credential store")
}

fn installation_seed() -> Result<[u8; 32]> {
    let entry = key_entry()?;
    match entry.get_secret() {
        Ok(secret) if secret.len() == 32 => secret
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid installation key")),
        Ok(_) => bail!(
            "the stored CLI installation key is invalid; remove it from your OS credential store and run `turtle login` again"
        ),
        Err(keyring::Error::NoEntry) => {
            let mut seed = [0_u8; 32];
            getrandom::fill(&mut seed)
                .map_err(|error| anyhow::anyhow!("generating an installation key: {error}"))?;
            entry
                .set_secret(&seed)
                .context("saving the installation key to your OS credential store")?;
            Ok(seed)
        }
        Err(error) => {
            Err(error).context("reading the installation key from your OS credential store")
        }
    }
}

fn canonical_json(value: &Value) -> String {
    match value {
        Value::Array(items) => format!(
            "[{}]",
            items
                .iter()
                .map(canonical_json)
                .collect::<Vec<_>>()
                .join(",")
        ),
        Value::Object(object) => {
            let mut keys: Vec<_> = object.keys().collect();
            keys.sort();
            let members = keys
                .into_iter()
                .map(|key| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(key).expect("string serialization"),
                        canonical_json(&object[key])
                    )
                })
                .collect::<Vec<_>>();
            format!("{{{}}}", members.join(","))
        }
        _ => serde_json::to_string(value).expect("JSON value serialization"),
    }
}

fn thumbprint(public_key: &[u8; 32]) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(
        URL_SAFE_NO_PAD.encode(public_key).as_bytes(),
    ))
}

fn verify_lease(envelope: &LeaseEnvelope, expected_thumbprint: &str) -> Result<()> {
    if envelope.key_id != KEY_ID {
        bail!(
            "the account lease uses an unsupported signing key; update Turtle Flow and try again"
        );
    }
    let payload = canonical_json(&envelope.payload);
    let public_bytes: [u8; 32] = STANDARD
        .decode(PUBLIC_KEY)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid embedded license key"))?;
    let key = VerifyingKey::from_bytes(&public_bytes)
        .context("loading the embedded license verification key")?;
    let signature = Signature::from_slice(&STANDARD.decode(&envelope.signature)?)
        .context("decoding the account lease signature")?;
    key.verify_strict(payload.as_bytes(), &signature)
        .context("the account lease signature is invalid")?;
    let bound = envelope
        .payload
        .get("installation_key_thumbprint")
        .and_then(Value::as_str);
    if bound != Some(expected_thumbprint) {
        bail!("the account lease belongs to a different CLI installation");
    }
    let expiry = envelope
        .payload
        .get("expires_at")
        .and_then(Value::as_str)
        .context("the account lease has no expiry")?;
    let expiry =
        OffsetDateTime::parse(expiry, &Rfc3339).context("the account lease expiry is invalid")?;
    if expiry <= OffsetDateTime::now_utc() {
        bail!("the account lease is expired; run `turtle login` to renew it");
    }
    Ok(())
}

fn save_lease(home: &crate::paths::Home, lease: &LeaseEnvelope) -> Result<()> {
    home.ensure()?;
    let path = home.root.join("account-lease.json");
    let temporary = home.root.join("account-lease.json.tmp");
    let bytes = serde_json::to_vec_pretty(lease)?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temporary)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(&bytes)?;
    file.sync_all()?;
    std::fs::rename(temporary, path).context("saving the account lease")?;
    Ok(())
}

pub fn report_status(home: &crate::paths::Home) {
    let path = home.root.join("account-lease.json");
    let Ok(contents) = std::fs::read(path) else {
        eprintln!(
            "No current Turtle Flow account license is linked. Run `turtle login` to connect this CLI. Your local Free workflow and data remain available."
        );
        return;
    };
    let Ok(lease) = serde_json::from_slice::<LeaseEnvelope>(&contents) else {
        eprintln!(
            "The cached account license could not be read. Run `turtle login` to link this CLI again. Local Free use remains available."
        );
        return;
    };
    let seed =
        key_entry().and_then(|entry| entry.get_secret().context("reading the installation key"));
    let Ok(seed) = seed else {
        eprintln!(
            "No current account license is linked. Run `turtle login` to connect this CLI. Your local Free workflow and data remain available."
        );
        return;
    };
    let seed: [u8; 32] = match seed.try_into() {
        Ok(seed) => seed,
        Err(_) => {
            eprintln!(
                "The CLI installation key is invalid. Run `turtle login` to link this CLI again; local Free use remains available."
            );
            return;
        }
    };
    let public_key = ed25519_dalek::SigningKey::from_bytes(&seed)
        .verifying_key()
        .to_bytes();
    if let Err(error) = verify_lease(&lease, &thumbprint(&public_key)) {
        eprintln!(
            "No current account license is available ({}). Run `turtle login`; local Free use remains available.",
            error
        );
    }
}

fn launch_browser(url: &str) -> bool {
    let command = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "cmd"
    } else {
        "xdg-open"
    };
    let args: &[&str] = if cfg!(target_os = "windows") {
        &["/C", "start", "", url]
    } else {
        &[url]
    };
    Command::new(command)
        .args(args)
        .status()
        .is_ok_and(|status| status.success())
}

pub async fn login(home: &crate::paths::Home) -> Result<()> {
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        bail!("`turtle login` requires an interactive terminal");
    }
    let seed = installation_seed()?;
    let signing = ed25519_dalek::SigningKey::from_bytes(&seed);
    let public_key = signing.verifying_key().to_bytes();
    let public_key_encoded = URL_SAFE_NO_PAD.encode(public_key);
    let thumbprint = thumbprint(&public_key);
    let device_name = hostname();
    let client = reqwest::Client::builder()
        .user_agent(concat!("turtle-flow/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let response = client
        .post(format!("{PORTAL}/api/cli/device/authorizations"))
        .json(&AuthorizationRequest {
            public_key: public_key_encoded,
            device_name: &device_name,
            platform: std::env::consts::OS,
            cli_version: env!("CARGO_PKG_VERSION"),
        })
        .send()
        .await
        .context("requesting a CLI authorization")?;
    if !response.status().is_success() {
        bail!(
            "the account service rejected this login request (HTTP {})",
            response.status()
        );
    }
    let authorization: AuthorizationResponse = response
        .json()
        .await
        .context("reading the authorization response")?;
    println!(
        "\nTo link this CLI, open {} and enter:\n\n    {}\n",
        authorization.verification_uri, authorization.user_code
    );
    if !launch_browser(&authorization.verification_uri) {
        println!("Open the link above in a browser where you are signed in to Turtle Flow.");
    }
    println!("Waiting for approval… (Ctrl-C to cancel)");
    let expires_at = tokio::time::Instant::now() + Duration::from_secs(authorization.expires_in);
    let mut interval = authorization.interval.max(5);
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => bail!("login cancelled"),
            _ = tokio::time::sleep(Duration::from_secs(interval)) => {}
        }
        if tokio::time::Instant::now() >= expires_at {
            bail!("the login request expired; run `turtle login` to try again");
        }
        let response = client
            .post(format!("{PORTAL}/api/cli/device/tokens"))
            .json(&json!({"device_code": authorization.device_code}))
            .send()
            .await
            .context("polling for CLI approval")?;
        let status = response.status();
        if status.is_success() {
            let value: Value = response
                .json()
                .await
                .context("reading the signed account lease")?;
            let lease: LeaseEnvelope = serde_json::from_value(
                value
                    .get("lease")
                    .cloned()
                    .context("missing lease in authorization response")?,
            )?;
            verify_lease(&lease, &thumbprint).context("verifying the returned account lease")?;
            save_lease(home, &lease)?;
            let expiry = lease.payload["expires_at"].as_str().unwrap_or("unknown");
            println!("Turtle Flow account license linked. Lease valid through {expiry}.");
            return Ok(());
        }
        let value: Value = response.json().await.unwrap_or(Value::Null);
        match value.get("error").and_then(Value::as_str) {
            Some("authorization_pending") => continue,
            Some("slow_down") => interval = interval.saturating_add(5),
            Some("access_denied") => bail!("the CLI authorization was denied"),
            Some("expired_token") => {
                bail!("the login request expired; run `turtle login` to try again")
            }
            _ if status == StatusCode::TOO_MANY_REQUESTS => interval = interval.saturating_add(5),
            _ => bail!("the account service could not complete login (HTTP {status})"),
        }
    }
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .unwrap_or_else(|_| "Turtle Flow CLI".to_owned())
        .chars()
        .take(100)
        .collect()
}

use std::io::IsTerminal;
