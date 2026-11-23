use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;

use super::{BackupService, SettingsService};
use crate::domain::activity::Actor;
use crate::domain::update::{
    Release, embedded_key, expected_hash, is_newer, parse_release, sha256_hex, verify_signature,
    windows_package,
};
use crate::domain::validation::invalid;
use crate::error::{DomainError, DomainResult};
use crate::infrastructure::updates::UpdateSource;

/// Where releases are published (the project's own repository).
pub const REPO: &str = "Az-Heda/Orbit";
const KEY_FILE: &str = include_str!("../../update-public-key.txt");
const MAX_ARCHIVE: usize = 200 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct UpdateInfo {
    pub current: String,
    pub enabled: bool,
    pub available: bool,
    pub skipped: bool,
    pub release: Option<Release>,
    /// None when the app can install the update itself; otherwise why it cannot.
    pub install_blocker: Option<String>,
}

/// Self-update for the plain-executable distribution: check GitHub for a newer release, verify a
/// signed checksum, then swap the running exe by renaming (with rollback). Never runs without a
/// signature that matches the embedded public key.
type Verifier = fn(&str, &[u8], &str) -> DomainResult<()>;

pub struct UpdateService {
    source: Arc<dyn UpdateSource>,
    settings: Arc<SettingsService>,
    backups: Arc<BackupService>,
    current: String,
    key: Option<String>,
    exe: PathBuf,
    can_swap: bool,
    verifier: Verifier,
    busy: AtomicBool,
}

impl UpdateService {
    pub fn new(
        source: Arc<dyn UpdateSource>,
        settings: Arc<SettingsService>,
        backups: Arc<BackupService>,
        exe: PathBuf,
    ) -> Self {
        Self {
            source,
            settings,
            backups,
            current: env!("CARGO_PKG_VERSION").to_string(),
            key: embedded_key(KEY_FILE),
            exe,
            can_swap: cfg!(windows),
            verifier: verify_signature,
            busy: AtomicBool::new(false),
        }
    }

    #[cfg(test)]
    fn for_test(mut self, current: &str, key: Option<&str>, can_swap: bool) -> Self {
        self.current = current.into();
        self.key = key.map(str::to_string);
        self.can_swap = can_swap;
        self
    }

    async fn flag<T: serde::de::DeserializeOwned>(&self, key: &str, default: T) -> T {
        match self.settings.get(key).await {
            Ok(Some(s)) => serde_json::from_str(&s.value).unwrap_or(default),
            _ => default,
        }
    }

    fn blocker(&self) -> Option<String> {
        if !self.can_swap {
            return Some("automatic install is only available for the Windows executable".into());
        }
        if self.key.is_none() {
            return Some("this build has no update signing key".into());
        }
        None
    }

    /// Look for a newer release. Without `force`, nothing is contacted while checking is off.
    pub async fn check(&self, force: bool) -> DomainResult<UpdateInfo> {
        let enabled = self.flag("update.check_enabled", true).await;
        let mut info = UpdateInfo {
            current: self.current.clone(),
            enabled,
            available: false,
            skipped: false,
            release: None,
            install_blocker: self.blocker(),
        };
        if !enabled && !force {
            return Ok(info);
        }
        let release = parse_release(&self.source.latest_release(REPO).await?, REPO)?;
        if is_newer(&self.current, &release.version) {
            let skipped: String = self.flag("update.skipped", String::new()).await;
            info.skipped = skipped == release.version;
            info.available = true;
            info.release = Some(release);
        }
        Ok(info)
    }

    pub async fn skip(&self, version: &str) -> DomainResult<()> {
        semver::Version::parse(version).map_err(|_| invalid("not a version"))?;
        self.settings
            .set(
                "update.skipped",
                &serde_json::to_string(version).unwrap_or_default(),
                Actor::User,
            )
            .await?;
        Ok(())
    }

    /// Download, verify and install the latest release in place of the running executable.
    /// On success the new exe is on disk at the old path; the caller relaunches it and exits.
    pub async fn install(&self) -> DomainResult<PathBuf> {
        if let Some(b) = self.blocker() {
            return Err(invalid(b));
        }
        if self.busy.swap(true, Ordering::SeqCst) {
            return Err(invalid("an update is already in progress"));
        }
        let r = self.install_inner().await;
        self.busy.store(false, Ordering::SeqCst);
        r
    }

    async fn install_inner(&self) -> DomainResult<PathBuf> {
        let key = self
            .key
            .clone()
            .ok_or_else(|| invalid("no update signing key"))?;
        let release = parse_release(&self.source.latest_release(REPO).await?, REPO)?;
        if !is_newer(&self.current, &release.version) {
            return Err(invalid("already up to date"));
        }
        let pkg = windows_package(&release)
            .ok_or_else(|| invalid("this release is not signed or has no Windows build"))?;

        let sums = self.source.download(&pkg.sums.url, 64 * 1024).await?;
        let sig = self.source.download(&pkg.signature.url, 8 * 1024).await?;
        let sig = String::from_utf8(sig).map_err(|_| invalid("malformed signature"))?;
        (self.verifier)(&key, &sums, &sig)?;
        let sums = String::from_utf8(sums).map_err(|_| invalid("malformed checksum list"))?;
        let want = expected_hash(&sums, &pkg.archive.name)
            .ok_or_else(|| invalid("the signed checksum list does not cover the download"))?;

        let zip = self.source.download(&pkg.archive.url, MAX_ARCHIVE).await?;
        if sha256_hex(&zip) != want {
            return Err(invalid(
                "the download does not match its checksum: refusing to install",
            ));
        }

        let staged = sibling(&self.exe, ".new");
        extract_exe(&zip, &staged)?;
        self.backups.backup_now("pre-update").await?;
        if let Err(e) = swap_executable(&self.exe, &staged) {
            let _ = std::fs::remove_file(&staged);
            return Err(e);
        }
        Ok(self.exe.clone())
    }
}

fn sibling(exe: &Path, suffix: &str) -> PathBuf {
    let mut s = exe.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

fn io(e: std::io::Error) -> DomainError {
    DomainError::Storage(format!("file error: {e}"))
}

/// Write the `orbit.exe` entry of the release zip to `dest`.
pub fn extract_exe(zip_bytes: &[u8], dest: &Path) -> DomainResult<()> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes))
        .map_err(|_| invalid("the download is not a valid zip file"))?;
    let mut entry = archive
        .by_name("orbit.exe")
        .map_err(|_| invalid("the download does not contain orbit.exe"))?;
    let mut out = Vec::new();
    entry
        .by_ref()
        .take(MAX_ARCHIVE as u64 + 1)
        .read_to_end(&mut out)
        .map_err(io)?;
    if out.is_empty() || out.len() > MAX_ARCHIVE {
        return Err(invalid("unexpected size of orbit.exe in the download"));
    }
    std::fs::write(dest, &out).map_err(|e| {
        DomainError::Validation(format!(
            "cannot write next to the app (is the folder read-only?): {e}"
        ))
    })
}

/// Replace `current` with `new` using renames only: a running exe can be renamed on Windows
/// but not overwritten. Rolls back if the second step fails.
pub fn swap_executable(current: &Path, new: &Path) -> DomainResult<()> {
    let old = sibling(current, ".old");
    let _ = std::fs::remove_file(&old);
    std::fs::rename(current, &old).map_err(|e| {
        DomainError::Validation(format!(
            "cannot replace the app (is the folder read-only?): {e}"
        ))
    })?;
    if let Err(e) = std::fs::rename(new, current) {
        let restored = std::fs::rename(&old, current);
        return Err(DomainError::Storage(format!(
            "update failed ({e}); {}",
            if restored.is_ok() {
                "the previous version was restored"
            } else {
                "RESTORE FAILED, rename orbit.exe.old back by hand"
            }
        )));
    }
    Ok(())
}

/// At startup: remove the previous version left behind by an update.
pub fn cleanup_old_exe(exe: &Path) {
    let _ = std::fs::remove_file(sibling(exe, ".old"));
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::sync::Mutex;

    use async_trait::async_trait;

    use super::*;
    use crate::domain::update::sha256_hex;
    use crate::infrastructure::database::{SqliteSettingsRepository, open};

    struct Fake {
        latest: String,
        files: Vec<(String, Vec<u8>)>,
        hits: Mutex<usize>,
    }
    #[async_trait]
    impl UpdateSource for Fake {
        async fn latest_release(&self, _repo: &str) -> DomainResult<String> {
            *self.hits.lock().unwrap() += 1;
            Ok(self.latest.clone())
        }
        async fn download(&self, url: &str, _max: usize) -> DomainResult<Vec<u8>> {
            self.files
                .iter()
                .find(|(n, _)| url.ends_with(n.as_str()))
                .map(|(_, b)| b.clone())
                .ok_or_else(|| DomainError::External("404".into()))
        }
    }

    fn zip_with(name: &str, data: &[u8]) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(vec![]);
        let mut w = zip::ZipWriter::new(&mut buf);
        w.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        w.write_all(data).unwrap();
        w.finish().unwrap();
        buf.into_inner()
    }

    fn release_json(tag: &str) -> String {
        let a = |n: &str| {
            format!(
                r#"{{"name":"{n}","browser_download_url":"https://github.com/{REPO}/releases/download/{tag}/{n}","size":1}}"#
            )
        };
        format!(
            r#"{{"tag_name":"{tag}","html_url":"https://github.com/{REPO}/releases/tag/{tag}","body":"n","assets":[{},{},{}]}}"#,
            a(&format!("orbit-{tag}-windows-x64.zip")),
            a("SHA256SUMS"),
            a("SHA256SUMS.minisig")
        )
    }

    struct Env {
        svc: UpdateService,
        dir: PathBuf,
        fake: Arc<Fake>,
    }

    async fn setup(files: Vec<(String, Vec<u8>)>, tag: &str) -> Env {
        let dir = std::env::temp_dir().join(format!("orbit-update-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let pool = open(&dir.join("orbit.db")).await.unwrap();
        let settings = Arc::new(SettingsService::new(Arc::new(
            SqliteSettingsRepository::new(pool.clone()),
        )));
        let backups = Arc::new(BackupService::new(
            pool,
            dir.clone(),
            settings.clone(),
            Arc::new(chrono::Utc::now),
        ));
        let exe = dir.join("orbit.exe");
        std::fs::write(&exe, b"old binary").unwrap();
        let fake = Arc::new(Fake {
            latest: release_json(tag),
            files,
            hits: Mutex::new(0),
        });
        let svc = UpdateService::new(fake.clone(), settings, backups, exe);
        Env { svc, dir, fake }
    }

    #[tokio::test]
    async fn check_reports_newer_versions_honours_opt_out_and_skip() {
        let e = setup(vec![], "v9.0.0").await;
        let svc = e.svc.for_test("1.0.0", None, true);
        let i = svc.check(false).await.unwrap();
        assert!(i.available && !i.skipped && i.release.unwrap().version == "9.0.0");
        assert!(i.install_blocker.unwrap().contains("signing key"));

        svc.skip("9.0.0").await.unwrap();
        assert!(svc.check(false).await.unwrap().skipped);

        svc.settings
            .set("update.check_enabled", "false", Actor::User)
            .await
            .unwrap();
        let before = *e.fake.hits.lock().unwrap();
        let off = svc.check(false).await.unwrap();
        assert!(!off.available && !off.enabled);
        assert_eq!(
            *e.fake.hits.lock().unwrap(),
            before,
            "no network call while disabled"
        );
        assert!(
            svc.check(true).await.unwrap().available,
            "Check now still works"
        );

        let same = svc.for_test("9.0.0", None, true).check(true).await.unwrap();
        assert!(!same.available);
        let _ = std::fs::remove_dir_all(&e.dir);
    }

    // Test vector from the minisign-verify docs: signs the bytes "test".
    const KEY: &str = "RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
    const SIG: &str = "untrusted comment: signature from minisign secret key\nRUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=\ntrusted comment: timestamp:1633700835\tfile:test\tprehashed\nwLMDjy9FLAuxZ3q4NlEvkgtyhrr0gtTu6KC4KBJdITbbOeAi1zBIYo0v4iTgt8jJpIidRJnp94ABQkJAgAooBQ==";

    #[tokio::test]
    async fn install_refuses_without_a_key_on_other_platforms_and_on_bad_signatures() {
        // The signature only covers b"test", so a real checksum list can never verify here.
        let zip = zip_with("orbit.exe", b"new binary");
        let sums = format!("{}  orbit-v9.0.0-windows-x64.zip\n", sha256_hex(&zip));
        let files = vec![
            ("orbit-v9.0.0-windows-x64.zip".to_string(), zip),
            ("SHA256SUMS".to_string(), sums.into_bytes()),
            ("SHA256SUMS.minisig".to_string(), SIG.as_bytes().to_vec()),
        ];
        let e = setup(files, "v9.0.0").await;
        let exe = e.dir.join("orbit.exe");
        let svc = e.svc;
        let no_key = UpdateService {
            key: None,
            can_swap: true,
            ..clone_for(&svc)
        };
        assert!(no_key.install().await.is_err());
        let linux = UpdateService {
            key: Some(KEY.into()),
            can_swap: false,
            ..clone_for(&svc)
        };
        assert!(linux.install().await.is_err());
        let bad_sig = UpdateService {
            current: "1.0.0".into(),
            key: Some(KEY.into()),
            can_swap: true,
            ..clone_for(&svc)
        };
        let err = bad_sig.install().await.unwrap_err().to_string();
        assert!(err.contains("signature"), "{err}");
        assert_eq!(
            std::fs::read(&exe).unwrap(),
            b"old binary",
            "nothing was touched"
        );
        let _ = std::fs::remove_dir_all(&e.dir);
    }

    fn clone_for(s: &UpdateService) -> UpdateService {
        UpdateService {
            source: s.source.clone(),
            settings: s.settings.clone(),
            backups: s.backups.clone(),
            current: s.current.clone(),
            key: s.key.clone(),
            exe: s.exe.clone(),
            can_swap: s.can_swap,
            verifier: s.verifier,
            busy: AtomicBool::new(false),
        }
    }

    fn accept(_: &str, _: &[u8], _: &str) -> DomainResult<()> {
        Ok(())
    }

    fn package(zip: &[u8], sums_for: &[u8]) -> Vec<(String, Vec<u8>)> {
        let sums = format!("{}  orbit-v9.0.0-windows-x64.zip\n", sha256_hex(sums_for));
        vec![
            ("orbit-v9.0.0-windows-x64.zip".to_string(), zip.to_vec()),
            ("SHA256SUMS".to_string(), sums.into_bytes()),
            ("SHA256SUMS.minisig".to_string(), b"sig".to_vec()),
        ]
    }

    #[tokio::test]
    async fn full_install_flow_checks_hash_extracts_backs_up_and_swaps() {
        // Signature verification is covered by the real-vector tests; here it is stubbed so the
        // rest of the pipeline runs end to end.
        let zip = zip_with("orbit.exe", b"new binary");
        let e = setup(package(&zip, &zip), "v9.0.0").await;
        let mut svc = e.svc.for_test("1.0.0", Some(KEY), true);
        svc.verifier = accept;
        let path = svc.install().await.unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new binary");
        assert_eq!(
            std::fs::read(e.dir.join("orbit.exe.old")).unwrap(),
            b"old binary"
        );
        assert!(!e.dir.join("orbit.exe.new").exists());
        assert!(
            svc.backups
                .list()
                .unwrap()
                .iter()
                .any(|b| b.kind == "pre-update")
        );
        cleanup_old_exe(&path);
        assert!(!e.dir.join("orbit.exe.old").exists());
        let _ = std::fs::remove_dir_all(&e.dir);
    }

    #[tokio::test]
    async fn a_download_that_does_not_match_the_signed_checksum_is_never_installed() {
        let zip = zip_with("orbit.exe", b"new binary");
        let evil = zip_with("orbit.exe", b"malware");
        // The signed list vouches for `zip`, but the server hands out `evil`.
        let e = setup(package(&evil, &zip), "v9.0.0").await;
        let mut svc = e.svc.for_test("1.0.0", Some(KEY), true);
        svc.verifier = accept;
        let err = svc.install().await.unwrap_err().to_string();
        assert!(err.contains("checksum"), "{err}");
        assert_eq!(
            std::fs::read(e.dir.join("orbit.exe")).unwrap(),
            b"old binary"
        );
        assert!(!e.dir.join("orbit.exe.new").exists() && !e.dir.join("orbit.exe.old").exists());
        assert!(
            svc.install().await.is_err(),
            "and nothing is left half-done to retry from"
        );
        let _ = std::fs::remove_dir_all(&e.dir);
    }

    #[tokio::test]
    async fn extraction_and_swap_helpers() {
        let zip = zip_with("orbit.exe", b"new binary");
        let e = setup(vec![], "v9.0.0").await;
        let staged = e.dir.join("orbit.exe.new");
        extract_exe(&zip, &staged).unwrap();
        assert!(extract_exe(&zip_with("other.txt", b"x"), &e.dir.join("y")).is_err());
        assert!(extract_exe(b"not a zip", &e.dir.join("y")).is_err());
        swap_executable(&e.svc.exe, &staged).unwrap();
        assert_eq!(std::fs::read(&e.svc.exe).unwrap(), b"new binary");
        let _ = std::fs::remove_dir_all(&e.dir);
    }

    #[tokio::test]
    async fn failed_swap_restores_the_previous_version() {
        let e = setup(vec![], "v9.0.0").await;
        let missing = e.dir.join("does-not-exist.new");
        let err = swap_executable(&e.svc.exe, &missing)
            .unwrap_err()
            .to_string();
        assert!(err.contains("restored"), "{err}");
        assert_eq!(std::fs::read(&e.svc.exe).unwrap(), b"old binary");
        assert!(!e.dir.join("orbit.exe.old").exists());
        let _ = std::fs::remove_dir_all(&e.dir);
    }
}
