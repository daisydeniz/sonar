//! `sonar license keygen|issue|status|activate|deactivate` (T7.5, ADR-038).
//!
//! `status`, `activate` and `deactivate` work on the license state of the
//! data directory (`$SONAR_HOME` or Application Support), like the app.

use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use sonar_core::license::{self, Date, LicenseInfo, LicenseKind, LicenseStatus};
use sonar_core::paths;

/// `sonar license` actions.
#[derive(Debug, Subcommand)]
pub enum LicenseCommand {
    /// Generate a signing key pair: writes the secret key to `--out`
    /// (refuses to overwrite; mode 0600) and prints the public key to set
    /// as SONAR_LICENSE_PUBLIC_KEY when building a release.
    Keygen {
        /// File to write the secret key to (must not exist).
        #[arg(long, value_name = "FILE")]
        out: PathBuf,
    },
    /// Sign a license key for a customer; prints the key.
    Issue(IssueArgs),
    /// Show trial / license status (starts the trial on first use).
    Status {
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Verify a license key and store it.
    Activate {
        /// The key (`SONAR1-...`); whitespace is ignored, so a wrapped key
        /// may be given as several words.
        #[arg(required = true)]
        key: Vec<String>,
    },
    /// Remove the stored license key (the trial clock is not reset).
    Deactivate,
}

/// Arguments of `sonar license issue`.
#[derive(Debug, Args)]
pub struct IssueArgs {
    /// Secret key file written by `sonar license keygen`.
    #[arg(long, value_name = "FILE")]
    secret_key: PathBuf,
    /// Name or e-mail of the licensee.
    #[arg(long)]
    licensee: String,
    /// Opaque order/license id from the store (for support lookups).
    #[arg(long)]
    license_id: String,
    /// `perpetual` (default) or `subscription` (needs --expires).
    #[arg(long, default_value = "perpetual")]
    kind: LicenseKind,
    /// Last valid day, `YYYY-MM-DD`.
    #[arg(long, value_name = "DATE", value_parser = parse_date)]
    expires: Option<Date>,
    /// Issue date, `YYYY-MM-DD` (default: today, UTC).
    #[arg(long, value_name = "DATE", value_parser = parse_date)]
    issued: Option<Date>,
}

fn parse_date(s: &str) -> Result<Date, String> {
    Date::parse(s).map_err(|e| e.to_string())
}

/// Runs a license action.
pub fn run(command: &LicenseCommand) -> Result<()> {
    match command {
        LicenseCommand::Keygen { out } => keygen(out),
        LicenseCommand::Issue(args) => issue(args),
        LicenseCommand::Status { json } => status(*json),
        LicenseCommand::Activate { key } => activate(&key.join(" ")),
        LicenseCommand::Deactivate => {
            let dir = paths::data_dir().context("locating the Sonar data directory")?;
            license::deactivate(&dir)?;
            println!("license removed");
            Ok(())
        }
    }
}

fn keygen(out: &std::path::Path) -> Result<()> {
    let pair = license::generate_keypair()?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(out)
        .with_context(|| format!("creating {} (it must not exist yet)", out.display()))?;
    writeln!(file, "{}", pair.secret).with_context(|| format!("writing {}", out.display()))?;
    println!("{}", pair.public);
    eprintln!(
        "secret key written to {} (keep it private, never commit it); \
         build releases with SONAR_LICENSE_PUBLIC_KEY=<the line above>",
        out.display()
    );
    Ok(())
}

fn issue(a: &IssueArgs) -> Result<()> {
    let text = std::fs::read_to_string(&a.secret_key)
        .with_context(|| format!("reading {}", a.secret_key.display()))?;
    let secret = license::parse_secret_key(&text)?;
    let info = LicenseInfo {
        licensee: a.licensee.clone(),
        license_id: a.license_id.clone(),
        issued: a
            .issued
            .unwrap_or_else(|| Date::from_unix(license::now_unix())),
        kind: a.kind,
        expires: a.expires,
    };
    println!("{}", license::issue(&secret, &info)?);
    Ok(())
}

fn status(json: bool) -> Result<()> {
    let dir = paths::data_dir().context("locating the Sonar data directory")?;
    let status = license::status(&dir, license::now_unix())?;
    let dev = license::uses_dev_key();
    let (state, detail): (&str, Option<&LicenseInfo>) = match &status {
        LicenseStatus::Trial { .. } => ("trial", None),
        LicenseStatus::TrialExpired { .. } => ("trial_expired", None),
        LicenseStatus::Licensed { info } => ("licensed", Some(info)),
        LicenseStatus::LicenseExpired { info } => ("license_expired", Some(info)),
    };
    if json {
        let mut v = serde_json::json!({ "state": state, "dev_key": dev });
        match &status {
            LicenseStatus::Trial { days_left, ends_at } => {
                v["days_left"] = (*days_left).into();
                v["ends_at"] = (*ends_at).into();
            }
            LicenseStatus::TrialExpired { ended_at } => v["ended_at"] = (*ended_at).into(),
            _ => {}
        }
        if let Some(i) = detail {
            v["licensee"] = i.licensee.clone().into();
            v["license_id"] = i.license_id.clone().into();
            v["kind"] = i.kind.as_str().into();
            v["issued"] = i.issued.to_string().into();
            v["expires"] = i.expires.map(|d| d.to_string()).into();
        }
        println!("{v}");
        return Ok(());
    }
    match &status {
        LicenseStatus::Trial { days_left, .. } => println!("trial: {days_left} day(s) left"),
        LicenseStatus::TrialExpired { .. } => println!("trial expired"),
        LicenseStatus::Licensed { info } => print_info("licensed", info),
        LicenseStatus::LicenseExpired { info } => print_info("license expired", info),
    }
    if dev {
        println!("note: dev license key in use (build without SONAR_LICENSE_PUBLIC_KEY)");
    }
    Ok(())
}

fn print_info(head: &str, i: &LicenseInfo) {
    let exp = i.expires.map_or("never".to_owned(), |d| d.to_string());
    println!(
        "{head}: {} (id {}, {}, issued {}, expires {exp})",
        i.licensee,
        i.license_id,
        i.kind.as_str(),
        i.issued
    );
}

fn activate(key: &str) -> Result<()> {
    if key.trim().is_empty() {
        bail!("empty license key");
    }
    let dir = paths::data_dir().context("locating the Sonar data directory")?;
    let info = license::activate(&dir, key, license::now_unix())?;
    print_info("activated", &info);
    Ok(())
}
