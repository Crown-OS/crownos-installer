//! `/proc/cpuinfo` parsing.

use std::collections::HashSet;

/// The x86-64 targets there are package builds for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Microarch {
    #[default]
    Generic,
    V3,
    /// AMD Zen 4 and later.
    Znver4,
}

/// Every flag x86-64-v2 and v3 require, as the kernel spells them: `pni` is
/// SSE3 and `abm` is LZCNT.
const V3_FLAGS: &[&str] = &[
    "cx16", "lahf_lm", "popcnt", "pni", "sse4_1", "sse4_2", "ssse3", "avx", "avx2", "bmi1", "bmi2",
    "f16c", "fma", "abm", "movbe", "xsave",
];

const V4_FLAGS: &[&str] = &["avx512f", "avx512bw", "avx512cd", "avx512dq", "avx512vl"];

const AMD_VENDOR: &str = "AuthenticAMD";

/// Zen 3 shares this family with Zen 4, but has none of [`V4_FLAGS`].
const ZEN4_FAMILY: u32 = 0x19;

fn field<'a>(cpuinfo: &'a str, name: &str) -> Option<&'a str> {
    cpuinfo
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(key, _)| key.trim() == name)
        .map(|(_, value)| value.trim())
}

/// The most specific target the first CPU listed runs.
pub fn microarch(cpuinfo: &str) -> Microarch {
    let flags: HashSet<&str> = field(cpuinfo, "flags")
        .unwrap_or_default()
        .split_whitespace()
        .collect();
    let has_all = |required: &[&str]| required.iter().all(|flag| flags.contains(flag));
    let is_zen4_or_later = field(cpuinfo, "vendor_id") == Some(AMD_VENDOR)
        && field(cpuinfo, "cpu family").and_then(|family| family.parse().ok()) >= Some(ZEN4_FAMILY);

    match (has_all(V3_FLAGS), is_zen4_or_later && has_all(V4_FLAGS)) {
        (true, true) => Microarch::Znver4,
        (true, false) => Microarch::V3,
        (false, _) => Microarch::Generic,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const V3: &str = "pni ssse3 fma cx16 sse4_1 sse4_2 movbe popcnt xsave avx f16c lahf_lm abm \
        bmi1 avx2 bmi2";
    const V4: &str = "avx512f avx512dq avx512cd avx512bw avx512vl";

    fn cpu(vendor: &str, family: u32, flags: &str) -> String {
        format!(
            "processor\t: 0\nvendor_id\t: {vendor}\ncpu family\t: {family}\n\
             flags\t\t: fpu sse2 {flags}\nvmx flags\t: vnmi\n"
        )
    }

    #[test]
    fn zen4_and_zen5_run_znver4_builds() {
        assert_eq!(
            microarch(&cpu(AMD_VENDOR, 25, &format!("{V3} {V4}"))),
            Microarch::Znver4
        );
        assert_eq!(
            microarch(&cpu(AMD_VENDOR, 26, &format!("{V3} {V4}"))),
            Microarch::Znver4
        );
    }

    #[test]
    fn zen3_runs_v3_builds() {
        assert_eq!(microarch(&cpu(AMD_VENDOR, 25, V3)), Microarch::V3);
    }

    #[test]
    fn avx512_on_intel_runs_v3_builds() {
        assert_eq!(
            microarch(&cpu("GenuineIntel", 6, &format!("{V3} {V4}"))),
            Microarch::V3
        );
    }

    #[test]
    fn a_cpu_without_avx2_stays_generic() {
        let sandy_bridge = "pni ssse3 cx16 sse4_1 sse4_2 popcnt xsave avx lahf_lm";
        assert_eq!(
            microarch(&cpu("GenuineIntel", 6, sandy_bridge)),
            Microarch::Generic
        );
    }

    #[test]
    fn unreadable_cpuinfo_stays_generic() {
        assert_eq!(microarch(""), Microarch::Generic);
    }
}
