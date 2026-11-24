//! Line by line, what changed between two versions of a note.

/// How a line differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Same,
    Added,
    Removed,
}

/// Past this many line pairs, the changed middle is shown as removed and
/// added whole rather than matched line by line.
const MOST_PAIRS: usize = 2_000_000;

/// The lines of `old` and `new`, each marked same, added or removed.
pub fn diff_lines(old: &str, new: &str) -> Vec<(Change, String)> {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..].iter().rev().zip(b[prefix..].iter().rev()).take_while(|(x, y)| x == y).count();
    let (a_mid, b_mid) = (&a[prefix..a.len() - suffix], &b[prefix..b.len() - suffix]);
    let line = |change: Change| move |text: &&str| (change, text.to_string());
    let mut out: Vec<(Change, String)> = a[..prefix].iter().map(line(Change::Same)).collect();
    if a_mid.len().saturating_mul(b_mid.len()) > MOST_PAIRS {
        out.extend(a_mid.iter().map(line(Change::Removed)));
        out.extend(b_mid.iter().map(line(Change::Added)));
    } else {
        // The longest common subsequence, counted from the end.
        let (n, m) = (a_mid.len(), b_mid.len());
        let width = m + 1;
        let mut common = vec![0u32; (n + 1) * width];
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                common[i * width + j] = if a_mid[i] == b_mid[j] {
                    common[(i + 1) * width + j + 1] + 1
                } else {
                    common[(i + 1) * width + j].max(common[i * width + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < n && j < m {
            if a_mid[i] == b_mid[j] {
                out.push((Change::Same, a_mid[i].to_string()));
                i += 1;
                j += 1;
            } else if common[(i + 1) * width + j] >= common[i * width + j + 1] {
                out.push((Change::Removed, a_mid[i].to_string()));
                i += 1;
            } else {
                out.push((Change::Added, b_mid[j].to_string()));
                j += 1;
            }
        }
        out.extend(a_mid[i..].iter().map(line(Change::Removed)));
        out.extend(b_mid[j..].iter().map(line(Change::Added)));
    }
    out.extend(a[a.len() - suffix..].iter().map(line(Change::Same)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use Change::*;

    #[test]
    fn marks_changed_lines() {
        let old = "# Title\nkeep\nold line\nmiddle\nend";
        let new = "# Title\nkeep\nmiddle\nnew line\nend";
        let lines = diff_lines(old, new);
        let marks: Vec<(Change, &str)> = lines.iter().map(|(c, t)| (*c, t.as_str())).collect();
        assert_eq!(
            marks,
            [
                (Same, "# Title"),
                (Same, "keep"),
                (Removed, "old line"),
                (Same, "middle"),
                (Added, "new line"),
                (Same, "end")
            ]
        );
        assert_eq!(diff_lines("", "a"), [(Added, "a".to_string())]);
        assert_eq!(diff_lines("a\nb", "a\nb"), [(Same, "a".to_string()), (Same, "b".to_string())]);
    }
}
