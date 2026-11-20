//! Captures every installer page and state for the CrownOS-Design Figma file. Run both appearances
//! with `../crownui/capture/scripts/capture-app.sh .`; output lands in `$CROWN_DESIGN_DIR`.
//!
//! The installer is a binary, so its modules are compiled into this example directly. Nothing
//! here probes the real machine, and no executor runs, so neither the network worker nor the
//! install ever starts.

#![allow(dead_code, unused_imports)]

#[path = "../../src/autoinstall.rs"]
mod autoinstall;
#[path = "../../src/cmdline.rs"]
mod cmdline;
#[path = "../../src/engine/mod.rs"]
mod engine;
#[path = "../../src/live.rs"]
mod live;
#[path = "../../src/network/mod.rs"]
mod network;
#[path = "../../src/oobe.rs"]
mod oobe;
#[path = "../../src/payload.rs"]
mod payload;
#[path = "../../src/provenance.rs"]
mod provenance;
#[path = "../../src/ui/mod.rs"]
mod ui;
#[path = "../../src/util/mod.rs"]
mod util;

mod machine;
mod pages;
mod session;

use capture::Carrier;

fn main() -> anyhow::Result<()> {
    let mut carrier = Carrier::new(
        "Installer",
        "crownos-installer: the kiosk install wizard (intro, network, disk, space, partition \
         editor, account, apps, confirmation, progress, done and blocked pages), captured \
         headlessly from the current source.",
    );
    pages::capture_all(&mut carrier)?;
    let path = carrier.write(None)?;
    println!("{}", path.display());
    capture::finish()
}
