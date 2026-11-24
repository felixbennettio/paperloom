//! How a map is coloured, as XMind's themes colour it: the paper, the
//! centre, the main topics (filled with their branch's colour or outlined
//! in it), the lines of relations and summaries, and the branch colours in
//! turn. A note picks one with its `mindmap-theme` property.

/// A map's colours, `#rrggbb`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapTheme {
    pub id: &'static str,
    pub background: &'static str,
    /// The centre topic's fill and text.
    pub centre: &'static str,
    pub centre_text: &'static str,
    /// Main topics filled with their branch's colour (text in
    /// `centre_text`), or outlined in it on `topic`.
    pub filled: bool,
    /// The fill of outlined topics, and text on them and under them.
    pub topic: &'static str,
    pub text: &'static str,
    /// Relations, summaries and their labels' borders.
    pub line: &'static str,
    /// The main branches' colours, in turn.
    pub branches: [&'static str; 6],
}

/// The themes, the default first.
pub const MAP_THEMES: [MapTheme; 6] = [
    MapTheme {
        id: "classic",
        background: "#ffffff",
        centre: "#4f46e5",
        centre_text: "#ffffff",
        filled: false,
        topic: "#ffffff",
        text: "#1f2328",
        line: "#6e7781",
        branches: ["#4f46e5", "#e5484d", "#30a46c", "#f76b15", "#00a2c7", "#8e4ec6"],
    },
    MapTheme {
        id: "ocean",
        background: "#f2f8fc",
        centre: "#0b4f6c",
        centre_text: "#ffffff",
        filled: true,
        topic: "#ffffff",
        text: "#0b2a3a",
        line: "#5b7a8c",
        branches: ["#0b84a5", "#1d6fa3", "#2a9d8f", "#3a5f8a", "#127b8c", "#4062bb"],
    },
    MapTheme {
        id: "forest",
        background: "#f5f8f1",
        centre: "#1b4332",
        centre_text: "#ffffff",
        filled: true,
        topic: "#ffffff",
        text: "#1b2f22",
        line: "#6b7f6e",
        branches: ["#2d6a4f", "#588157", "#386641", "#6a8d3a", "#40916c", "#7b6d3f"],
    },
    MapTheme {
        id: "sunset",
        background: "#fff8f2",
        centre: "#7a1f3d",
        centre_text: "#ffffff",
        filled: true,
        topic: "#ffffff",
        text: "#3b1d14",
        line: "#9a7b6f",
        branches: ["#d1495b", "#e07a2d", "#b5651d", "#c2185b", "#8e3b8f", "#d35400"],
    },
    MapTheme {
        id: "mono",
        background: "#ffffff",
        centre: "#1f2328",
        centre_text: "#ffffff",
        filled: false,
        topic: "#ffffff",
        text: "#1f2328",
        line: "#8c959f",
        branches: ["#1f2328", "#424a53", "#57606a", "#6e7781", "#424a53", "#57606a"],
    },
    MapTheme {
        id: "dark",
        background: "#0d1117",
        centre: "#e6edf3",
        centre_text: "#0d1117",
        filled: false,
        topic: "#161b22",
        text: "#e6edf3",
        line: "#8b949e",
        branches: ["#58a6ff", "#ff7b72", "#3fb950", "#d29922", "#39c5cf", "#bc8cff"],
    },
];

impl MapTheme {
    /// The theme with an ID, any case; the first one otherwise.
    pub fn named(id: &str) -> &'static MapTheme {
        let id = id.trim();
        MAP_THEMES.iter().find(|t| t.id.eq_ignore_ascii_case(id)).unwrap_or(&MAP_THEMES[0])
    }

    /// The colour of the branch a topic (`0.2.1`) is on; the first for the
    /// centre.
    pub fn branch(&self, id: &str) -> &'static str {
        let branch = id.split('.').nth(1).and_then(|b| b.parse::<usize>().ok()).unwrap_or(0);
        self.branches[branch % self.branches.len()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_themes_and_branch_colours() {
        assert_eq!(MapTheme::named(" Ocean ").id, "ocean");
        assert_eq!(MapTheme::named("nonsense").id, "classic");
        let classic = MapTheme::named("");
        assert_eq!(classic.branch("0"), classic.branches[0]);
        assert_eq!(classic.branch("0.7.2"), classic.branches[1]);
        for theme in &MAP_THEMES {
            let colours =
                [theme.background, theme.centre, theme.centre_text, theme.topic, theme.text, theme.line];
            assert!(colours.iter().chain(&theme.branches).all(|c| c.len() == 7 && c.starts_with('#')));
        }
    }
}
