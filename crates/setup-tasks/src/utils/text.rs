/// The last `count` lines of `text`, without the trailing line break.
pub fn last_lines(text: &str, count: usize) -> &str {
    let trimmed = text.trim_end();
    match count
        .checked_sub(1)
        .and_then(|skipped| trimmed.rmatch_indices('\n').nth(skipped))
    {
        Some((index, _)) => &trimmed[index + 1..],
        None if count == 0 => "",
        None => trimmed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_the_last_lines() {
        assert_eq!(last_lines("a\nb\nc\nd\n", 2), "c\nd");
        assert_eq!(last_lines("a\nb\n", 5), "a\nb");
        assert_eq!(last_lines("a\nb\n", 0), "");
        assert_eq!(last_lines("", 3), "");
    }
}
