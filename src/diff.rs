//! キーごとの比較と、その表示。

use std::fmt::Write as _;

use crate::Entries;

/// 1 つのキーの比較結果の種類。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// 左にだけある
    Removed,
    /// 右にだけある
    Added,
    /// 値が違う
    Changed,
    /// 一致
    Same,
}

impl Mark {
    fn symbol(self) -> char {
        match self {
            Self::Removed => '-',
            Self::Added => '+',
            Self::Changed => '~',
            Self::Same => '=',
        }
    }

    /// 端末の文字色 (SGR のパラメータ)
    fn color(self) -> &'static str {
        match self {
            Self::Removed => "31",
            Self::Added => "32",
            Self::Changed => "33",
            Self::Same => "2",
        }
    }
}

/// 1 つのキーの比較結果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub mark: Mark,
    pub key: String,
    /// 固定の文言だけ。`&'static str` なので、ファイルから読んだ物は入れられない。
    pub note: &'static str,
}

const EMPTY: &str = "（空）";

/// キーごとに比べ、削除 → 追加 → 変更 → 一致 の順に並べる。各グループ内はファイルの並び順。
pub fn compare(left: &Entries, right: &Entries, keys_only: bool) -> Vec<Change> {
    let change = |mark: Mark, key: &str, note: &'static str| Change {
        mark,
        key: key.to_owned(),
        note,
    };
    let mut removed = Vec::new();
    let mut changed = Vec::new();
    let mut same = Vec::new();
    for (key, a) in left.iter() {
        match right.get(key) {
            None => removed.push(change(
                Mark::Removed,
                key,
                if a.is_empty() { EMPTY } else { "" },
            )),
            Some(b) if keys_only || a.same_value(b) => {
                let note = if a.is_empty() && b.is_empty() {
                    EMPTY
                } else {
                    ""
                };
                same.push(change(Mark::Same, key, note));
            }
            Some(_) if a.is_empty() => changed.push(change(Mark::Changed, key, "空 → 値あり")),
            Some(b) if b.is_empty() => changed.push(change(Mark::Changed, key, "値あり → 空")),
            Some(_) => changed.push(change(Mark::Changed, key, "値が違う")),
        }
    }
    let added = right
        .iter()
        .filter(|(key, _)| left.get(key).is_none())
        .map(|(key, b)| change(Mark::Added, key, if b.is_empty() { EMPTY } else { "" }));
    removed
        .into_iter()
        .chain(added)
        .chain(changed)
        .chain(same)
        .collect()
}

/// 比較結果を表示用の文字列にする。
pub fn render(
    left_path: &str,
    right_path: &str,
    changes: &[Change],
    show_same: bool,
    color: bool,
) -> String {
    let paint = |code: &str, text: &str| {
        if color {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_owned()
        }
    };

    let shown: Vec<&Change> = changes
        .iter()
        .filter(|c| show_same || c.mark != Mark::Same)
        .collect();
    let width = shown.iter().map(|c| c.key.len()).max().unwrap_or(0);
    let mut out = String::new();
    // String への書き込みは失敗しない
    let _ = writeln!(out, "{}", paint("1", &format!("--- {left_path}")));
    let _ = writeln!(out, "{}", paint("1", &format!("+++ {right_path}")));
    for c in &shown {
        let line = format!("{} {:<width$}  {}", c.mark.symbol(), c.key, c.note);
        let _ = writeln!(out, "  {}", paint(c.mark.color(), line.trim_end()));
    }

    let count = |mark: Mark| changes.iter().filter(|c| c.mark == mark).count();
    let (added, removed, changed) = (
        count(Mark::Added),
        count(Mark::Removed),
        count(Mark::Changed),
    );
    let total = added + removed + changed;
    let mut summary = if total > 0 {
        format!("差分 {total}（追加 {added} / 削除 {removed} / 変更 {changed}）")
    } else {
        "差分なし".to_owned()
    };
    if show_same {
        let _ = write!(summary, "、一致 {}", count(Mark::Same));
    }
    if !shown.is_empty() {
        out.push('\n');
    }
    out + &summary + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_only_when_requested() {
        let changes = [Change {
            mark: Mark::Added,
            key: "A".to_owned(),
            note: "",
        }];
        assert!(render("a", "b", &changes, false, true).contains('\x1b'));
        assert!(!render("a", "b", &changes, false, false).contains('\x1b'));
    }
}
