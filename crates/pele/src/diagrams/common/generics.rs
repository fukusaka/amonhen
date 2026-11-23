//! `List~int~` を `List<int>` に（pele の `src/diagrams/common/generics.ts`）。

fn tildes(text: &str) -> usize {
    text.matches('~').count()
}

/// チルダを外から対にする: 最初と最後、2 番目と最後から 2 番目。
fn pair_tildes(text: &str) -> String {
    let count = tildes(text);
    if count <= 1 {
        return text.to_string();
    }
    let lead = !count.is_multiple_of(2) && text.starts_with('~');
    let mut chars: Vec<char> = text.chars().collect();
    let at: Vec<usize> = chars
        .iter()
        .enumerate()
        .skip(usize::from(lead))
        .filter(|&(_, &c)| c == '~')
        .map(|(i, _)| i)
        .collect();
    let (mut lo, mut hi) = (0, at.len() - 1);
    while lo < hi {
        chars[at[lo]] = '<';
        chars[at[hi]] = '>';
        lo += 1;
        hi -= 1;
    }
    chars.into_iter().collect()
}

/// `List~int~` を `List<int>` に。カンマのまわりは Mermaid の parseGenericTypes のくせのまま。
pub fn parse_generic_types(input: &str) -> String {
    if !input.contains('~') {
        return input.to_string();
    }
    // `split(/(,)/)`: カンマも 1 つの組として残す。
    let mut sets: Vec<&str> = Vec::new();
    let mut from = 0;
    for (i, _) in input.match_indices(',') {
        sets.push(&input[from..i]);
        sets.push(",");
        from = i + 1;
    }
    sets.push(&input[from..]);
    let mut output: Vec<String> = Vec::new();
    let mut i = 0;
    while i < sets.len() {
        let mut set = sets[i].to_string();
        if set == ","
            && i > 0
            && i + 1 < sets.len()
            && tildes(sets[i - 1]) == 1
            && tildes(sets[i + 1]) == 1
        {
            set = format!("{},{}", sets[i - 1], sets[i + 1]);
            i += 1;
            output.pop();
        }
        output.push(pair_tildes(&set));
        i += 1;
    }
    output.concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_tildes() {
        assert_eq!(parse_generic_types("List~int~"), "List<int>");
        assert_eq!(parse_generic_types("Map~K, V~"), "Map<K, V>");
        assert_eq!(parse_generic_types("List~List~int~~"), "List<List<int>>");
        assert_eq!(parse_generic_types("~a~b~"), "~a<b>");
        assert_eq!(parse_generic_types("a~b"), "a~b");
        assert_eq!(parse_generic_types("x~a~,y~b~"), "x<a>,y<b>");
        assert_eq!(parse_generic_types("plain"), "plain");
    }
}
