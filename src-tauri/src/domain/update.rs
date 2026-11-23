//! Pure logic of the self-updater: release parsing, version comparison, signed-checksum
//! verification and picking the right asset. No I/O, so it is unit-tested directly.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::domain::validation::invalid;
use crate::error::DomainResult;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Release {
    pub tag: String,
    pub version: String,
    pub notes: String,
    pub page_url: String,
    pub assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct RawAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

#[derive(Deserialize)]
struct RawRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<RawAsset>,
}

/// Parse a GitHub "release" JSON object. Downloads must come from this repository's release
/// area over https; anything else is dropped, so a tampered API answer cannot point elsewhere.
pub fn parse_release(json: &str, repo: &str) -> DomainResult<Release> {
    let raw: RawRelease =
        serde_json::from_str(json).map_err(|_| invalid("unexpected release data"))?;
    if raw.draft || raw.prerelease {
        return Err(invalid("the latest release is not a stable release"));
    }
    let version = raw.tag_name.trim_start_matches('v').to_string();
    semver::Version::parse(&version).map_err(|_| invalid("the release tag is not a version"))?;
    let prefix = format!("https://github.com/{repo}/releases/download/");
    let page = format!("https://github.com/{repo}/");
    if !raw.html_url.starts_with(&page) {
        return Err(invalid("unexpected release page"));
    }
    Ok(Release {
        tag: raw.tag_name,
        version,
        notes: raw.body.unwrap_or_default().chars().take(4000).collect(),
        page_url: raw.html_url,
        assets: raw
            .assets
            .into_iter()
            .filter(|a| a.browser_download_url.starts_with(&prefix) && !a.name.contains('/'))
            .map(|a| Asset {
                name: a.name,
                url: a.browser_download_url,
                size: a.size,
            })
            .collect(),
    })
}

/// Is `latest` strictly newer than `current`? Unparseable versions are never "newer".
pub fn is_newer(current: &str, latest: &str) -> bool {
    let p = |v: &str| semver::Version::parse(v.trim_start_matches('v')).ok();
    matches!((p(current), p(latest)), (Some(c), Some(l)) if l > c)
}

pub struct Package<'a> {
    pub archive: &'a Asset,
    pub sums: &'a Asset,
    pub signature: &'a Asset,
}

/// The Windows zip plus its signed checksum list. All three must be present.
pub fn windows_package(release: &Release) -> Option<Package<'_>> {
    let find = |f: &dyn Fn(&str) -> bool| release.assets.iter().find(|a| f(&a.name));
    Some(Package {
        archive: find(&|n| n.ends_with("-windows-x64.zip"))?,
        sums: find(&|n| n == "SHA256SUMS")?,
        signature: find(&|n| n == "SHA256SUMS.minisig")?,
    })
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Verify the minisign signature of the checksum list against the embedded public key.
/// `public_key` is the base64 key (the second line of a `.pub` file).
pub fn verify_signature(public_key: &str, sums: &[u8], signature: &str) -> DomainResult<()> {
    let key = minisign_verify::PublicKey::from_base64(public_key.trim())
        .map_err(|_| invalid("the embedded update key is invalid"))?;
    let sig = minisign_verify::Signature::decode(signature)
        .map_err(|_| invalid("the update signature is malformed"))?;
    key.verify(sums, &sig, false)
        .map_err(|_| invalid("the update signature does not match: refusing to install"))
}

/// The expected SHA-256 of `file` from a `sha256sum`-style list ("<hex>  <name>").
pub fn expected_hash(sums: &str, file: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let (hash, name) = l.split_once(char::is_whitespace)?;
        let name = name.trim().trim_start_matches('*');
        (name == file && hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| hash.to_ascii_lowercase())
    })
}

/// The key to trust, from the embedded key file (comment lines are ignored); None when the
/// project has not configured one yet.
pub fn embedded_key(file: &str) -> Option<String> {
    file.lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("untrusted comment") && !l.starts_with('#'))
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPO: &str = "o/r";
    fn release_json(tag: &str, extra: &str) -> String {
        format!(
            r#"{{"tag_name":"{tag}","html_url":"https://github.com/o/r/releases/tag/{tag}","body":"notes",
            "assets":[
              {{"name":"orbit-{tag}-windows-x64.zip","browser_download_url":"https://github.com/o/r/releases/download/{tag}/orbit-{tag}-windows-x64.zip","size":10}},
              {{"name":"SHA256SUMS","browser_download_url":"https://github.com/o/r/releases/download/{tag}/SHA256SUMS","size":1}},
              {{"name":"SHA256SUMS.minisig","browser_download_url":"https://github.com/o/r/releases/download/{tag}/SHA256SUMS.minisig","size":1}}
              {extra}]}}"#
        )
    }

    #[test]
    fn version_comparison_is_semver() {
        assert!(is_newer("1.2.0", "v1.10.0"));
        assert!(!is_newer("1.2.0", "v1.2.0"));
        assert!(!is_newer("2.0.0", "1.9.9"));
        assert!(is_newer("1.0.0-beta.1", "1.0.0"));
        assert!(!is_newer("1.0.0", "latest"), "garbage is never an update");
    }

    #[test]
    fn release_parsing_keeps_only_assets_from_this_repo() {
        let evil = r#",{"name":"x.zip","browser_download_url":"https://evil.example/o/r/releases/download/v1/x.zip"}"#;
        let r = parse_release(&release_json("v1.3.0", evil), REPO).unwrap();
        assert_eq!(r.version, "1.3.0");
        assert_eq!(r.assets.len(), 3, "foreign asset dropped");
        assert!(windows_package(&r).is_some());
        assert!(parse_release(&release_json("v1.3.0", ""), "other/repo").is_err());
        assert!(
            parse_release(
                r#"{"tag_name":"nightly","html_url":"https://github.com/o/r/x"}"#,
                REPO
            )
            .is_err()
        );
        assert!(
            parse_release(
                r#"{"tag_name":"v1.0.0","html_url":"https://github.com/o/r/x","prerelease":true}"#,
                REPO
            )
            .is_err()
        );
        let mut partial = r;
        partial.assets.retain(|a| a.name != "SHA256SUMS.minisig");
        assert!(
            windows_package(&partial).is_none(),
            "unsigned releases are not installable"
        );
    }

    #[test]
    fn checksum_lookup_and_hashing() {
        let sums = format!(
            "{}  orbit.zip\n{} *other.zip\n",
            sha256_hex(b"abc"),
            "0".repeat(64)
        );
        assert_eq!(
            expected_hash(&sums, "orbit.zip").unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(expected_hash(&sums, "other.zip").is_some());
        assert!(expected_hash(&sums, "missing.zip").is_none());
        assert!(expected_hash("zz  orbit.zip", "orbit.zip").is_none());
    }

    const KEY: &str = "RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
    const SIG: &str = "untrusted comment: signature from minisign secret key\nRUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=\ntrusted comment: timestamp:1633700835\tfile:test\tprehashed\nwLMDjy9FLAuxZ3q4NlEvkgtyhrr0gtTu6KC4KBJdITbbOeAi1zBIYo0v4iTgt8jJpIidRJnp94ABQkJAgAooBQ==";

    #[test]
    fn signature_must_match_content_and_key() {
        verify_signature(KEY, b"test", SIG).unwrap();
        assert!(verify_signature(KEY, b"tampered", SIG).is_err());
        assert!(verify_signature(KEY, b"test", "garbage").is_err());
        assert!(verify_signature("not a key", b"test", SIG).is_err());
    }

    #[test]
    fn embedded_key_skips_comments_and_may_be_absent() {
        assert_eq!(
            embedded_key("untrusted comment: minisign public key X\nRWQabc\n").as_deref(),
            Some("RWQabc")
        );
        assert_eq!(embedded_key("# no key yet\n\n"), None);
        assert_eq!(embedded_key(""), None);
    }
}
