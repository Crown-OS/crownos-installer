//! Reading a systemd preset file.

/// The units that the `enable` lines of a systemd preset file name, in file
/// order. Blank lines and `#` or `;` comments are skipped.
pub fn enabled_units(preset: &str) -> impl Iterator<Item = &str> {
    preset.lines().filter_map(|line| {
        let mut words = line.split_whitespace();
        match (words.next(), words.next()) {
            (Some("enable"), Some(unit)) => Some(unit),
            _ => None,
        }
    })
}

/// The record of applied preset lines that crownos-updated reads: one unit
/// for each line.
pub fn applied_presets_record<'a>(units: impl IntoIterator<Item = &'a str>) -> String {
    units.into_iter().flat_map(|unit| [unit, "\n"]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_only_the_enable_lines() {
        let preset = "# CrownOS\nenable NetworkManager.service\n\n; old\ndisable foo.service\n  enable  snapper-cleanup.timer  \nenable\n";
        assert_eq!(
            enabled_units(preset).collect::<Vec<_>>(),
            ["NetworkManager.service", "snapper-cleanup.timer"]
        );
    }

    #[test]
    fn the_record_has_one_unit_on_each_line() {
        assert_eq!(
            applied_presets_record(["a.service", "b.timer"]),
            "a.service\nb.timer\n"
        );
        assert_eq!(applied_presets_record([]), "");
    }
}
