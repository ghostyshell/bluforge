//! Automatic ripping decisions.
//!
//! The watcher and the app share one rule for which titles an automatic rip
//! takes, so a disc behaves the same whether it is inserted while the CLI is
//! watching or while the desktop app is open.

use crate::disc::DiscInfo;

/// Which titles an automatic rip takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickMode {
    /// The main feature (libbluray's main title, else the longest).
    Main,
    /// Every title at least the configured minimum length.
    All,
}

impl PickMode {
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "all" | "every" => Self::All,
            _ => Self::Main,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::All => "all",
        }
    }
}

/// Title indexes to rip, in playback order.
///
/// `main` always returns one title: the one libbluray marked main, otherwise
/// the longest. `all` filters by length so menus and trailers are skipped.
pub fn pick_titles(info: &DiscInfo, mode: PickMode, min_secs: u32) -> Vec<u32> {
    match mode {
        PickMode::Main => {
            let marked = info
                .main_title
                .and_then(|index| info.titles.iter().find(|t| t.index == index))
                .or_else(|| info.titles.iter().find(|t| t.is_main))
                .or_else(|| {
                    info.titles
                        .iter()
                        .max_by(|a, b| a.duration_secs.total_cmp(&b.duration_secs))
                });
            marked.map(|t| vec![t.index]).unwrap_or_default()
        }
        PickMode::All => {
            let floor = min_secs.max(60) as f64;
            let picked: Vec<u32> = info
                .titles
                .iter()
                .filter(|t| t.duration_secs >= floor)
                .map(|t| t.index)
                .collect();
            if !picked.is_empty() {
                return picked;
            }
            // A short single-title disc (home video, music) still gets ripped.
            pick_titles(info, PickMode::Main, min_secs)
        }
    }
}

/// A stable identity for a disc, used to avoid ripping the same one twice.
///
/// The disc name alone is not enough: a season disc may share a name with
/// another. Title count and total runtime separate them without needing an
/// AACS ID on unencrypted discs.
pub fn disc_key(info: &DiscInfo) -> String {
    let total: u64 = info
        .titles
        .iter()
        .map(|t| t.duration_secs.round() as u64)
        .sum();
    format!("{}|{}|{}", info.name, info.titles.len(), total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disc::Title;

    fn title(index: u32, secs: f64, main: bool) -> Title {
        Title {
            index,
            playlist: index,
            duration_secs: secs,
            chapters: 1,
            is_main: main,
            resolution: None,
            dynamic_range: None,
            audio_streams: 1,
        }
    }

    fn disc(titles: Vec<Title>, main_title: Option<u32>) -> DiscInfo {
        DiscInfo {
            name: "TEST".into(),
            path: "/tmp/test".into(),
            aacs: false,
            aacs_ready: true,
            aacs_error: 0,
            titles,
            main_title,
            library: "1.5.0".into(),
            video_codec: None,
            audio_codec: None,
            audio_lang: None,
        }
    }

    #[test]
    fn main_prefers_the_marked_title() {
        let d = disc(
            vec![title(0, 100.0, false), title(1, 9000.0, true)],
            Some(1),
        );
        assert_eq!(pick_titles(&d, PickMode::Main, 600), vec![1]);
    }

    #[test]
    fn main_falls_back_to_the_longest() {
        let d = disc(vec![title(0, 120.0, false), title(1, 9000.0, false)], None);
        assert_eq!(pick_titles(&d, PickMode::Main, 600), vec![1]);
    }

    #[test]
    fn all_skips_menu_length_titles() {
        let d = disc(
            vec![
                title(0, 30.0, false),
                title(1, 2400.0, true),
                title(2, 2500.0, false),
            ],
            Some(1),
        );
        assert_eq!(pick_titles(&d, PickMode::All, 600), vec![1, 2]);
    }

    #[test]
    fn all_keeps_a_short_disc_above_the_floor() {
        let d = disc(vec![title(0, 300.0, true)], Some(0));
        assert_eq!(pick_titles(&d, PickMode::All, 600), vec![0]);
    }

    #[test]
    fn a_disc_key_changes_with_the_title_set() {
        let a = disc(vec![title(0, 100.0, true)], Some(0));
        let mut b = disc(vec![title(0, 100.0, true), title(1, 50.0, false)], Some(0));
        assert_ne!(disc_key(&a), disc_key(&b));
        b.titles.pop();
        assert_eq!(disc_key(&a), disc_key(&b));
    }
}
