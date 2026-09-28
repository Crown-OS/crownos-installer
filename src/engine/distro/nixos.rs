//! NixOS: the install is a configuration, and `nixos-install` builds it.
//!
//! Every choice the imperative distributions apply one command at a time —
//! locale, keymap, time zone, hostname, the account, swap, the boot loader and
//! the desktop — is written into `/etc/nixos/crownos.nix` instead. The one thing
//! that is not is the password: it would land in the world-readable Nix store,
//! so it is set with `chpasswd` inside the installed system afterwards.

use crate::engine::exec::Cmd;
use crate::oobe::Locale;

/// The channel the installed system follows.
const NIXPKGS: &str = "github:NixOS/nixpkgs/nixos-26.05";
const STATE_VERSION: &str = "26.05";

/// The flake that provides the CrownOS packages and NixOS module.
const CROWNOS_FLAKE: &str = "github:Crown-OS/crownos-iso?dir=packaging/nix";

/// The attribute the installed flake names the machine by. Fixed rather than
/// the hostname so `nixos-rebuild switch --flake /etc/nixos#crownos` keeps
/// working after a rename.
const CONFIGURATION_NAME: &str = "crownos";

/// A shared Windows ESP holds a few hundred megabytes; each generation kept on
/// it costs one kernel and one initrd.
const BOOT_GENERATIONS_ON_ESP: u32 = 2;

const FLAKE: &str = r#"{
  inputs = {
    nixpkgs.url = "@NIXPKGS@";
    crownos.url = "@CROWNOS@";
    crownos.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { nixpkgs, crownos, ... }: {
    nixosConfigurations.@NAME@ = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        ./hardware-configuration.nix
        ./crownos.nix
        crownos.nixosModules.default
      ];
    };
  };
}
"#;

/// What the configuration is generated from.
pub struct Choices<'a> {
    pub hostname: &'a str,
    pub username: &'a str,
    pub full_name: Option<&'a str>,
    pub autologin: bool,
    pub locale: &'a Locale,
    pub swap_file_bytes: Option<u64>,
    pub esp_rel: &'a str,
}

/// `/etc/nixos/crownos.nix`.
pub fn configuration(choices: &Choices<'_>) -> String {
    let user = nix_string(choices.username);
    let description = choices
        .full_name
        .map(|name| format!("    description = {};\n", nix_string(name)))
        .unwrap_or_default();
    let autologin = if choices.autologin {
        format!("  services.getty.autologinUser = {user};\n")
    } else {
        String::new()
    };
    let swap = choices
        .swap_file_bytes
        .map(|bytes| {
            let mebibytes = (bytes / (1024 * 1024)).max(1);
            format!("  swapDevices = [ {{ device = \"/swapfile\"; size = {mebibytes}; }} ];\n")
        })
        .unwrap_or_default();

    format!(
        "{{ ... }}:\n\
         {{\n\
         \x20 boot.loader.systemd-boot.enable = true;\n\
         \x20 boot.loader.systemd-boot.configurationLimit = {BOOT_GENERATIONS_ON_ESP};\n\
         \x20 boot.loader.efi.canTouchEfiVariables = true;\n\
         \x20 boot.loader.efi.efiSysMountPoint = {esp};\n\
         \n\
         \x20 networking.hostName = {hostname};\n\
         \x20 networking.networkmanager.enable = true;\n\
         \x20 i18n.defaultLocale = {locale};\n\
         \x20 console.keyMap = {keymap};\n\
         \x20 time.timeZone = {timezone};\n\
         {swap}\
         \n\
         \x20 users.users.${{{user}}} = {{\n\
         \x20   isNormalUser = true;\n\
         {description}\
         \x20   extraGroups = [ \"wheel\" \"networkmanager\" ];\n\
         \x20 }};\n\
         {autologin}\
         \n\
         \x20 services.crownos.enable = true;\n\
         \x20 nix.settings.experimental-features = [ \"nix-command\" \"flakes\" ];\n\
         \x20 system.stateVersion = \"{STATE_VERSION}\";\n\
         }}\n",
        esp = nix_string(choices.esp_rel),
        hostname = nix_string(choices.hostname),
        locale = nix_string(&choices.locale.locale),
        keymap = nix_string(&choices.locale.keymap),
        timezone = nix_string(&choices.locale.timezone),
    )
}

/// `/etc/nixos/flake.nix`.
pub fn flake() -> String {
    FLAKE
        .replace("@NIXPKGS@", NIXPKGS)
        .replace("@CROWNOS@", CROWNOS_FLAKE)
        .replace("@NAME@", CONFIGURATION_NAME)
}

/// Detect the hardware and filesystems, then lay the generated files over it.
pub fn write_configuration(root: &str, configuration: &str) -> Vec<Cmd> {
    vec![
        install_tool("nixos-generate-config", &["--root", root]),
        Cmd::WriteFile {
            path: format!("{root}/etc/nixos/crownos.nix"),
            contents: configuration.to_owned(),
        },
        Cmd::WriteFile {
            path: format!("{root}/etc/nixos/flake.nix"),
            contents: flake(),
        },
        // `nixos-generate-config` writes one; the flake's modules replace it.
        Cmd::run(
            "rm",
            &["-f", "--", &format!("{root}/etc/nixos/configuration.nix")],
        ),
    ]
}

pub fn install(root: &str) -> Vec<Cmd> {
    vec![install_tool(
        "nixos-install",
        &[
            "--root",
            root,
            "--no-root-passwd",
            "--no-channel-copy",
            "--flake",
            &format!("{root}/etc/nixos#{CONFIGURATION_NAME}"),
        ],
    )]
}

/// The program and arguments that run `chpasswd` inside the installed system.
pub fn chpasswd(root: &str) -> (String, Vec<String>) {
    (
        "nix".to_owned(),
        install_tool_args("nixos-enter", &["--root", root, "--", "chpasswd"]),
    )
}

/// Run one of `nixos-install-tools` from the live image's Nix, which is not
/// NixOS and so does not have them on `PATH`.
fn install_tool(tool: &str, args: &[&str]) -> Cmd {
    Cmd::Run {
        program: "nix".to_owned(),
        args: install_tool_args(tool, args),
    }
}

fn install_tool_args(tool: &str, args: &[&str]) -> Vec<String> {
    [
        "--extra-experimental-features",
        "nix-command flakes",
        "shell",
        &format!("{NIXPKGS}#nixos-install-tools"),
        "--command",
        tool,
    ]
    .into_iter()
    .chain(args.iter().copied())
    .map(str::to_owned)
    .collect()
}

/// A Nix string literal. `${` would otherwise start an interpolation.
fn nix_string(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace("${", "\\${");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choices(locale: &Locale) -> Choices<'_> {
        Choices {
            hostname: "crownos",
            username: "crown",
            full_name: Some("Crown \"The\" ${User}"),
            autologin: true,
            locale,
            swap_file_bytes: Some(8 * 1024 * 1024 * 1024),
            esp_rel: "/efi",
        }
    }

    #[test]
    fn user_text_cannot_break_out_of_its_string() {
        let locale = Locale::default();
        let config = configuration(&choices(&locale));
        assert!(
            config.contains(r#"description = "Crown \"The\" \${User}";"#),
            "{config}"
        );
    }

    #[test]
    fn the_configuration_carries_every_choice_but_the_password() {
        let locale = Locale::default();
        let config = configuration(&choices(&locale));
        for expected in [
            "boot.loader.efi.efiSysMountPoint = \"/efi\";",
            "networking.hostName = \"crownos\";",
            "i18n.defaultLocale = \"en_US.UTF-8\";",
            "time.timeZone = \"UTC\";",
            "users.users.${\"crown\"} = {",
            "services.getty.autologinUser = \"crown\";",
            "size = 8192;",
            "services.crownos.enable = true;",
        ] {
            assert!(config.contains(expected), "missing `{expected}`:\n{config}");
        }
        assert!(!config.to_lowercase().contains("password"));
    }

    #[test]
    fn the_flake_names_the_configuration_nixos_install_builds() {
        let root = "/mnt";
        let command = install(root)[0].to_string();
        assert!(command.contains(&format!("{root}/etc/nixos#{CONFIGURATION_NAME}")));
        assert!(flake().contains(&format!("nixosConfigurations.{CONFIGURATION_NAME}")));
        assert!(!flake().contains('@'));
    }
}
