//! Tool version pin checker.
//!
//! Compares an installed version string against the declared pin under
//! one of four strategies:
//! - `exact`: installed == declared
//! - `minor`: same major+minor, installed >= declared
//! - `major`: same major, installed >= declared
//! - `rolling`: any version accepted

use semver::Version;
use serde::Serialize;

use crate::config::VersionPinDecl;
use crate::error::{Error, Result};

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct VersionCheckOutcome {
    pub tool: String,
    pub declared: String,
    pub installed: String,
    pub strategy: String,
    pub ok: bool,
    pub message: Option<String>,
}

pub struct VersionChecker<'a> {
    pins: &'a [VersionPinDecl],
}

impl<'a> VersionChecker<'a> {
    pub fn new(pins: &'a [VersionPinDecl]) -> Self {
        Self { pins }
    }

    pub fn show(&self) -> &'a [VersionPinDecl] {
        self.pins
    }

    pub fn check_installed(&self, tool: &str, installed: &str) -> Result<VersionCheckOutcome> {
        let pin = self.pins.iter().find(|p| p.tool == tool).ok_or_else(|| {
            Error::PolicyVersionFailure {
                message: format!("tool '{tool}' not declared in [[policy.versions]]"),
            }
        })?;
        let ok = match pin.strategy.as_str() {
            "rolling" => true,
            "exact" => installed == pin.version,
            "minor" => satisfies_minor(&pin.version, installed)?,
            "major" => satisfies_major(&pin.version, installed)?,
            other => {
                return Err(Error::PolicyVersionFailure {
                    message: format!("unknown strategy '{other}'"),
                });
            }
        };
        Ok(VersionCheckOutcome {
            tool: tool.to_string(),
            declared: pin.version.clone(),
            installed: installed.to_string(),
            strategy: pin.strategy.clone(),
            ok,
            message: if ok {
                None
            } else {
                Some(format!(
                    "installed {installed} does not satisfy {} {}",
                    pin.strategy, pin.version
                ))
            },
        })
    }
}

fn satisfies_minor(declared: &str, installed: &str) -> Result<bool> {
    let d = parse(declared)?;
    let i = parse(installed)?;
    Ok(i.major == d.major && i.minor == d.minor && i >= d)
}

fn satisfies_major(declared: &str, installed: &str) -> Result<bool> {
    let d = parse(declared)?;
    let i = parse(installed)?;
    Ok(i.major == d.major && i >= d)
}

fn parse(v: &str) -> Result<Version> {
    Version::parse(v).map_err(|e| Error::PolicyVersionFailure {
        message: format!("version '{v}' is not SemVer: {e}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pins() -> Vec<VersionPinDecl> {
        vec![
            VersionPinDecl {
                tool: "exact-tool".into(),
                version: "1.2.3".into(),
                strategy: "exact".into(),
                install_url: None,
            },
            VersionPinDecl {
                tool: "minor-tool".into(),
                version: "1.2.3".into(),
                strategy: "minor".into(),
                install_url: None,
            },
            VersionPinDecl {
                tool: "major-tool".into(),
                version: "1.2.3".into(),
                strategy: "major".into(),
                install_url: None,
            },
            VersionPinDecl {
                tool: "rolling-tool".into(),
                version: "*".into(),
                strategy: "rolling".into(),
                install_url: None,
            },
        ]
    }

    #[test]
    fn exact_matches_only_equal() {
        let p = pins();
        let c = VersionChecker::new(&p);
        assert!(c.check_installed("exact-tool", "1.2.3").unwrap().ok);
        assert!(!c.check_installed("exact-tool", "1.2.4").unwrap().ok);
    }

    #[test]
    fn minor_accepts_same_minor_higher_patch() {
        let p = pins();
        let c = VersionChecker::new(&p);
        assert!(c.check_installed("minor-tool", "1.2.3").unwrap().ok);
        assert!(c.check_installed("minor-tool", "1.2.99").unwrap().ok);
        assert!(!c.check_installed("minor-tool", "1.3.0").unwrap().ok);
        assert!(!c.check_installed("minor-tool", "1.2.2").unwrap().ok);
    }

    #[test]
    fn major_accepts_same_major() {
        let p = pins();
        let c = VersionChecker::new(&p);
        assert!(c.check_installed("major-tool", "1.2.3").unwrap().ok);
        assert!(c.check_installed("major-tool", "1.9.0").unwrap().ok);
        assert!(!c.check_installed("major-tool", "2.0.0").unwrap().ok);
        assert!(!c.check_installed("major-tool", "1.2.2").unwrap().ok);
    }

    #[test]
    fn rolling_accepts_anything() {
        let p = pins();
        let c = VersionChecker::new(&p);
        assert!(c.check_installed("rolling-tool", "99.99.99").unwrap().ok);
    }

    #[test]
    fn unknown_tool_errors() {
        let p = pins();
        let c = VersionChecker::new(&p);
        assert!(c.check_installed("not-declared", "1.0.0").is_err());
    }
}
