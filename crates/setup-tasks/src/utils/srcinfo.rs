//! Reads the dependencies from the `.SRCINFO` file of an AUR package.

const DEPENDENCY_KEYS: [&str; 4] = [
    "depends",
    "makedepends",
    "depends_x86_64",
    "makedepends_x86_64",
];

/// The package names in the `depends` and `makedepends` lines, without
/// version constraints, each name one time, in the order of the file. A value
/// that is not a valid package name is ignored.
pub fn dependencies(srcinfo: &str) -> Vec<&str> {
    let mut names: Vec<&str> = Vec::new();
    for line in srcinfo.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if !DEPENDENCY_KEYS.contains(&key.trim()) {
            continue;
        }
        let name = value
            .split(['<', '>', '='])
            .next()
            .unwrap_or_default()
            .trim();
        if is_package_name(name) && !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// True when `name` uses only the characters of a pacman package name and
/// starts with a letter or a digit.
fn is_package_name(name: &str) -> bool {
    name.starts_with(|first: char| first.is_ascii_lowercase() || first.is_ascii_digit())
        && name.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || "@._+-".contains(character)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOGLE_CHROME_SRCINFO: &str = "pkgbase = google-chrome
\tpkgdesc = The popular web browser by Google (Stable Channel)
\tpkgver = 131.0.6778.85
\tarch = x86_64
\tdepends = alsa-lib
\tdepends = gtk3
\tdepends = nss>=3.26
\tdepends = libcups
\toptdepends = pipewire: WebRTC desktop sharing under Wayland
\tmakedepends = patchelf
\tdepends_x86_64 = gtk3
\tprovides = google-chrome

pkgname = google-chrome
";

    #[test]
    fn depends_and_makedepends_without_versions() {
        assert_eq!(
            dependencies(GOOGLE_CHROME_SRCINFO),
            ["alsa-lib", "gtk3", "nss", "libcups", "patchelf"]
        );
    }

    #[test]
    fn values_that_are_not_package_names_are_ignored() {
        let srcinfo = "\tdepends = --overwrite=*\n\tdepends = gtk3; rm -rf /\n\tdepends = libc++\n\tmakedepends = Qt\n\tdepends = -\n";
        assert_eq!(dependencies(srcinfo), ["libc++"]);
    }

    #[test]
    fn a_package_without_dependencies_has_none() {
        assert!(dependencies("pkgbase = hello\n\tpkgver = 1\n").is_empty());
    }
}
