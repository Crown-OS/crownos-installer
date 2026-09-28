//! Where this installer's instructions came from — and therefore which of two
//! installers it is.
//!
//! There are exactly two ways a user arrives at this program, and they want
//! almost nothing in common:
//!
//! * **From `crownos-windows-installer`.** Windows shrank a volume, staged an
//!   archiso payload, wrote `oobe.json` next to it and arranged a one-shot boot
//!   into it. Every decision has already been made and written down; this
//!   program's job is to check the disk still matches that description, ask for
//!   a password, and carry it out.
//! * **From a flashed image.** The user wrote the ISO to a stick and booted it.
//!   There is no `oobe.json`, because nothing wrote one. Every decision the
//!   payload would have carried has to be asked here instead.
//!
//! # The rule
//!
//! **If the configuration file is there, this came from the Windows installer.
//! If it is not, it is a flashed image.** That is the whole test, and it is a
//! test of the file rather than of the kernel command line on purpose — the file
//! is the thing that carries the decisions, and a boot with a marker but no file
//! has no decisions to act on however it got here.
//!
//! # What still blocks, and why it is only two cases
//!
//! Missing configuration is not an error any more; it is the other mode. Two
//! things are still errors, and both are the same shape: **we found a
//! configuration and cannot trust it.**
//!
//! * [`Blocked::Malformed`] — the file is there and does not parse. Falling back
//!   to the ISO flow here would be the worst of the three outcomes: the user
//!   asked for a specific dual-boot layout, something wrote a file describing
//!   it, and we would be quietly offering to partition the disk from scratch
//!   instead. A corrupted description of a disk is not the absence of a
//!   description.
//! * [`Blocked::Mismatch`] — the file and the kernel command line name different
//!   staging partitions, so they came from two different runs of the Windows
//!   installer. Two disagreeing accounts of which partition is the installer
//!   medium is precisely the situation in which guessing deletes the wrong one.
//!
//! Everything else — no marker, no `oobe_file=`, a marker whose file has gone —
//! resolves to [`Provenance::Iso`] with a note saying so. The note matters: a
//! *staged* boot whose payload could not be found is a real bug on the Windows
//! side, and it used to be a dead end with a good error message. Now it is a
//! working installer that still says what happened, which is strictly better
//! than either a dead end or silence.

use crate::cmdline::CmdlineArgs;
use crate::oobe::Oobe;
use crate::payload::{self, LoadError, Medium};

/// Which of the two installers this is.
#[derive(Clone, Debug, PartialEq)]
pub enum Provenance {
    /// A configuration file was found. It says what to install and where.
    WindowsInstaller {
        oobe: Box<Oobe>,
        /// Where it was read from, for the log and the summary.
        source: String,
    },
    /// No configuration file. The user flashed the image, so this program asks
    /// the questions the file would have answered.
    Iso,
}

impl Provenance {
    /// The payload, if there is one.
    pub fn oobe(&self) -> Option<&Oobe> {
        match self {
            Self::WindowsInstaller { oobe, .. } => Some(oobe),
            Self::Iso => None,
        }
    }

    pub fn is_iso(&self) -> bool {
        matches!(self, Self::Iso)
    }

    /// What to call this mode in the interface.
    pub fn name(&self) -> &'static str {
        match self {
            Self::WindowsInstaller { .. } => "Continuing from Windows",
            Self::Iso => "Installing from a CrownOS image",
        }
    }
}

/// A provenance, plus what was tried to establish it.
#[derive(Clone, Debug)]
pub struct Detected {
    pub provenance: Provenance,
    /// Everything the resolver tried, in order, and what happened — plus, on the
    /// ISO path, why there is no payload. Shown as diagnostics, and the first
    /// thing anyone debugging a boot will want.
    pub notes: Vec<String>,
}

/// We found instructions and will not act on them.
///
/// Only ever raised when a configuration file *was* located; see the module
/// docs for why absence is never in here.
#[derive(Clone, Debug)]
pub enum Blocked {
    Malformed { detail: String },
    Mismatch { detail: String, notes: Vec<String> },
}

impl Blocked {
    pub fn title(&self) -> &'static str {
        match self {
            Self::Malformed { .. } => "The handoff file could not be read",
            Self::Mismatch { .. } => "The handoff file does not match this boot",
        }
    }

    pub fn detail(&self) -> String {
        match self {
            Self::Malformed { detail } => format!(
                "{detail}\n\nThis boot carries a CrownOS handoff file, so it was \
                 arranged by the Windows installer — and the file describing what to \
                 do to the disk is not readable. The installer will not fall back to \
                 partitioning this disk from scratch, because a damaged description \
                 of a disk is not the same thing as no description at all. Run the \
                 Windows installer again."
            ),
            Self::Mismatch { detail, .. } => detail.clone(),
        }
    }

    pub fn notes(&self) -> Vec<String> {
        match self {
            Self::Malformed { .. } => Vec::new(),
            Self::Mismatch { notes, .. } => notes.clone(),
        }
    }
}

/// Work out which installer this is.
///
/// `medium` is how the payload is looked for; see
/// [`payload::locate`](crate::payload::locate) for the two strategies and why
/// there are two.
pub fn detect(args: &CmdlineArgs, medium: &dyn Medium) -> Result<Detected, Blocked> {
    match payload::locate(args, medium) {
        Ok(located) => {
            // A file was found. From here on, the only acceptable outcomes are
            // "act on it" and "stop and say why" — never "ignore it".
            let mut notes = located.attempts.clone();
            if let Err(error) = payload::check_stage_partuuid(args, &located.oobe) {
                return Err(Blocked::Mismatch {
                    detail: error.to_string(),
                    notes,
                });
            }
            notes.push(format!(
                "Handoff file read from {}. This install was set up by the CrownOS \
                 Windows installer.",
                located.source
            ));
            Ok(Detected {
                provenance: Provenance::WindowsInstaller {
                    oobe: Box::new(located.oobe),
                    source: located.source.to_string(),
                },
                notes,
            })
        }

        Err(LoadError::Malformed { source, message }) => Err(Blocked::Malformed {
            detail: format!("The file at {source} could not be parsed: {message}"),
        }),

        // `locate` returns this only from its own cross-check, which `detect`
        // reaches through the `Ok` arm above. Handled anyway rather than folded
        // into the ISO arm: it means two payloads disagreed, which must never
        // become "no payload".
        Err(error @ LoadError::StagePartuuidMismatch { .. }) => Err(Blocked::Mismatch {
            detail: error.to_string(),
            notes: Vec::new(),
        }),

        // No file. This is a flashed image — including the case where the
        // command line insists otherwise, which is a bug worth naming but not
        // one worth refusing to install over.
        Err(absent) => {
            let mut notes = attempts_of(&absent);
            notes.push(match &absent {
                LoadError::NotAStagedBoot => {
                    "No CrownOS handoff file, and nothing on the kernel command line \
                     claiming there should be one. This is a CrownOS image booted \
                     directly, so the installer will ask for everything itself."
                        .to_owned()
                }
                _ => "This boot claims to have been arranged by the CrownOS Windows \
                      installer, and its handoff file could not be found. The installer \
                      is continuing as though this were a plain CrownOS image, which \
                      means nothing about the disk is assumed — but the Windows side \
                      of this install did not finish properly, and the space it set \
                      aside may not be where you expect."
                    .to_owned(),
            });
            Ok(Detected {
                provenance: Provenance::Iso,
                notes,
            })
        }
    }
}

/// The resolver's own trail, where the error carries one.
fn attempts_of(error: &LoadError) -> Vec<String> {
    match error {
        LoadError::NoRouteToPayload { attempts } | LoadError::NotFound { attempts } => {
            attempts.clone()
        }
        _ => Vec::new(),
    }
}

// --- MARK: Tests ---

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmdline;
    use crate::payload::tests::{payload_json, FakeMedium, CMDLINE_PATH};

    /// A plain live ISO: nothing of ours on the command line, no file anywhere.
    /// This used to be `LoadError::NotAStagedBoot` and a dead end.
    #[test]
    fn a_flashed_image_is_the_iso_installer_and_not_an_error() {
        let args = cmdline::parse("initrd=\\arch\\boot\\x86_64\\initramfs-linux.img quiet");
        let detected = detect(&args, &FakeMedium::new()).expect("a flashed image installs");
        assert_eq!(detected.provenance, Provenance::Iso);
        assert!(detected.provenance.is_iso());
        assert!(detected.provenance.oobe().is_none());
        assert!(
            detected
                .notes
                .iter()
                .any(|note| note.contains("booted directly")),
            "{:?}",
            detected.notes
        );
    }

    /// A payload that is there and parses: the Windows path, unchanged.
    #[test]
    fn a_payload_that_parses_is_the_windows_installer() {
        let args = cmdline::parse(cmdline::tests::STAGED);
        let medium = FakeMedium::new().readable_now(CMDLINE_PATH, &payload_json());
        let detected = detect(&args, &medium).expect("a staged boot");
        match &detected.provenance {
            Provenance::WindowsInstaller { oobe, source } => {
                assert_eq!(oobe.schema_version, crate::oobe::SCHEMA_VERSION);
                assert!(source.contains("oobe.json"));
            }
            other => panic!("expected the Windows path, got {other:?}"),
        }
    }

    /// The case the module exists to get right: a staged boot whose payload has
    /// gone is an ISO install *with a note*, not a dead end. It used to be the
    /// latter.
    #[test]
    fn a_staged_boot_whose_payload_vanished_still_installs() {
        let args = cmdline::parse("crownos.stage=1 copytoram=y quiet");
        let detected = detect(&args, &FakeMedium::new()).expect("still installable");
        assert_eq!(detected.provenance, Provenance::Iso);
        assert!(
            detected
                .notes
                .iter()
                .any(|note| note.contains("did not finish properly")),
            "the user has to be told the Windows side failed: {:?}",
            detected.notes
        );
    }

    /// And the case it must *not* get wrong: a file that is there and is
    /// garbage. Falling through to the ISO flow would offer to repartition a
    /// disk whose intended layout we were just handed and could not read.
    #[test]
    fn a_malformed_payload_blocks_rather_than_falling_back_to_the_iso_flow() {
        let args = cmdline::parse(cmdline::tests::STAGED);
        let medium = FakeMedium::new().readable_now(CMDLINE_PATH, "{ this is not json");
        match detect(&args, &medium) {
            Err(blocked @ Blocked::Malformed { .. }) => {
                assert!(blocked.detail().contains("will not fall back"));
            }
            other => panic!("a malformed payload must block, got {other:?}"),
        }
    }

    /// Two disagreeing payloads must never resolve to "no payload".
    #[test]
    fn a_partuuid_mismatch_blocks() {
        let args = cmdline::parse(
            "crownos.stage=1 crownos.stage_partuuid=99999999-9999-9999-9999-999999999999 \
             oobe_file=/run/archiso/bootmnt/EFI/ArchStage/oobe.json",
        );
        let medium = FakeMedium::new().readable_now(CMDLINE_PATH, &payload_json());
        match detect(&args, &medium) {
            Err(Blocked::Mismatch { .. }) => {}
            other => panic!("expected a mismatch, got {other:?}"),
        }
    }
}
