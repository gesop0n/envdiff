//! dotenv のテキストを読む。生の値に触れるのはこのモジュールの中だけ。

use std::collections::HashMap;
use std::sync::LazyLock;

/// 値の比較にだけ使うハッシュの鍵。実行ごとに作り直すので、万一ハッシュが表示に
/// 紛れ込んでも、短い値を総当たりして元の値を割り出すことはできない。
static RUN_KEY: LazyLock<[u8; 32]> = LazyLock::new(|| {
    let mut key = [0; 32];
    getrandom::fill(&mut key).expect("OS の乱数を読めない");
    key
});

/// 1 つのキーについて、比較と表示に要るものだけを持つ。生の値は持たない。
///
/// Debug も Display も実装しないので、表示しようとするとコンパイルが通らない:
///
/// ```compile_fail
/// fn show(entry: &envdiff::Entry) -> String {
///     format!("{entry:?}")
/// }
/// ```
///
/// ```compile_fail
/// fn show(entry: &envdiff::Entry) -> String {
///     format!("{entry}")
/// }
/// ```
///
/// 使えるのは、空かどうかと、値が同じかどうかだけ:
///
/// ```
/// let (entries, _) = envdiff::parse_text("FOO=bar\n", "t.env");
/// let foo = entries.get("FOO").unwrap();
/// assert!(!foo.is_empty());
/// assert!(foo.same_value(foo));
/// ```
pub struct Entry {
    empty: bool,
    digest: Digest,
}

impl Entry {
    /// 値が空文字列か。
    pub fn is_empty(&self) -> bool {
        self.empty
    }

    /// 2 つの値が同じか。
    pub fn same_value(&self, other: &Entry) -> bool {
        self.digest == other.digest
    }
}

/// 値の鍵付きハッシュ。これも表示させないので Debug を実装しない。
#[derive(PartialEq, Eq)]
struct Digest(blake3::Hash);

impl Digest {
    fn of(value: &RawValue) -> Self {
        Self(blake3::keyed_hash(&RUN_KEY, value.0.as_bytes()))
    }
}

/// キーから Entry への表。並び順はファイルに最初に出てきた位置のまま。
#[derive(Default)]
pub struct Entries {
    list: Vec<(String, Entry)>,
    index: HashMap<String, usize>,
}

impl Entries {
    pub fn get(&self, key: &str) -> Option<&Entry> {
        self.index.get(key).map(|&i| &self.list[i].1)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Entry)> {
        self.list.iter().map(|(key, entry)| (key.as_str(), entry))
    }

    /// 既にあるキーなら値だけ差し替え、位置は最初のまま残す。
    fn insert(&mut self, key: String, entry: Entry) {
        if let Some(&i) = self.index.get(&key) {
            self.list[i].1 = entry;
        } else {
            self.index.insert(key.clone(), self.list.len());
            self.list.push((key, entry));
        }
    }
}

/// dotenv のテキストをキーごとの Entry にする。生の値はここで捨てる。
///
/// 同じキーが複数回あれば後ろの定義を使う。並び順は最初に出てきた位置のまま。
pub fn parse_text(text: &str, path: &str) -> (Entries, Vec<String>) {
    let (assignments, warnings) = scan(text, path);
    let mut entries = Entries::default();
    for Assignment { key, value } in assignments {
        let entry = Entry {
            empty: value.0.is_empty(),
            digest: Digest::of(&value),
        };
        entries.insert(key, entry);
    }
    (entries, warnings)
}

/// 生の値。Debug も Display も実装しないので、警告などの文言に紛れ込ませると
/// コンパイルが通らない。中身 (.0) を読んでよいのは parse_text とテストだけ。
struct RawValue(String);

struct Assignment {
    key: String,
    value: RawValue,
}

/// 値を読めなかった理由。値の中身は持たない。
enum BadValue {
    /// 閉じ引用符が無い。値がファイルの最後まで続いているかもしれない。
    Unclosed,
    /// 閉じ引用符の後ろに余分な文字がある。used は閉じ引用符の行までの行数。
    TrailingText { used: usize },
}

/// テキストを (キー, 生の値) の並びに分解し、警告と一緒に返す。
///
/// 読めない行は警告を積んで飛ばす。警告には行の中身を入れない。
///
/// 引用符の中は値なので、読めなかった値の途中の行をキーとして読み直すことはしない。
/// 読み直すと、値の断片 (base64 の `XXXX==` など) をキーと誤認して表示しうる。
fn scan(text: &str, path: &str) -> (Vec<Assignment>, Vec<String>) {
    // 改行は \n のほか \r\n と \r も認める
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = text.split('\n').collect();
    let mut assignments = Vec::new();
    let mut warnings = Vec::new();
    let mut defined_at: HashMap<&str, usize> = HashMap::new();
    let mut i = 0;
    while i < lines.len() {
        let lineno = i + 1;
        let stripped = lines[i].trim();
        if stripped.is_empty() || stripped.starts_with('#') {
            i += 1;
            continue;
        }

        let Some((key, rest)) = split_assignment(lines[i]) else {
            warnings.push(format!(
                "{path}:{lineno}: KEY=VALUE の形になっていない行を飛ばした"
            ));
            i += 1;
            continue;
        };

        let (value, used) = match read_value(rest, &lines, i) {
            Ok(read) => read,
            Err(BadValue::TrailingText { used }) => {
                // 閉じ引用符までが値なので、そこまでまとめて飛ばす
                warnings.push(format!(
                    "{path}:{lineno}: {key} の値を読めないので飛ばした (閉じ引用符の後ろに余分な文字がある)"
                ));
                i += used;
                continue;
            }
            Err(BadValue::Unclosed) => {
                // 後ろの行はすべて値の続きかもしれないので、最後まで読まない
                warnings.push(format!(
                    "{path}:{lineno}: {key} の値を読めないので、最後の行まで飛ばした (閉じ引用符が無い)"
                ));
                break;
            }
        };

        if let Some(previous) = defined_at.insert(key, lineno) {
            warnings.push(format!(
                "{path}:{lineno}: {key} が {previous} 行目にもあるので、後ろの定義を使う"
            ));
        }
        assignments.push(Assignment {
            key: key.to_owned(),
            value: RawValue(value),
        });
        i += used;
    }
    (assignments, warnings)
}

/// `KEY=...` の形の行なら (キー, `=` より後ろ) を返す。
///
/// 先頭の `export ` と `=` 前後の空白は許す。`export=1` や `export =1` は
/// export という名前のキーとして読む。
fn split_assignment(line: &str) -> Option<(&str, &str)> {
    let line = line.trim_start();
    if let Some(after) = line.strip_prefix("export") {
        let rest = after.trim_start();
        if rest.len() < after.len() {
            if let Some(found) = key_and_rest(rest) {
                return Some(found);
            }
        }
    }
    key_and_rest(line)
}

/// キーはシェルの変数名と同じ規則 (英数字と _、先頭は数字以外) に絞る。Node の dotenv は
/// . や - も許すが、許す文字を増やすほど、壊れた行に入っている値の断片をキーと誤認しやすくなる。
fn key_and_rest(s: &str) -> Option<(&str, &str)> {
    let end = s
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(s.len());
    let key = &s[..end];
    if key.is_empty() || key.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    let rest = s[end..].trim_start().strip_prefix('=')?;
    Some((key, rest))
}

/// `=` より後ろ (rest) を値として読む。返り値は (値, 使った行数)。
///
/// 引用符で囲まれた値は複数行にまたがってよいので、閉じ引用符が見つかるまで
/// lines[start] より後ろの行も読み進める。
fn read_value(rest: &str, lines: &[&str], start: usize) -> Result<(String, usize), BadValue> {
    let body = rest.trim_start();
    let quote = match body.chars().next() {
        Some(c @ ('\'' | '"')) => c,
        _ => return Ok((strip_comment(rest).trim().to_owned(), 1)),
    };

    let mut chunks = Vec::new();
    let mut segment = &body[1..];
    let mut i = start;
    let end = loop {
        if let Some(end) = closing_quote(segment, quote) {
            break end;
        }
        chunks.push(segment);
        i += 1;
        segment = lines.get(i).ok_or(BadValue::Unclosed)?;
    };
    chunks.push(&segment[..end]);
    let used = i - start + 1;

    let trailing = segment[end + 1..].trim_start();
    if !trailing.is_empty() && !trailing.starts_with('#') {
        return Err(BadValue::TrailingText { used });
    }

    let value = chunks.join("\n");
    let value = if quote == '"' {
        unescape(&value)
    } else {
        value
    };
    Ok((value, used))
}

/// 引用符なしの値から「空白 + #」以降のコメントを落とす。`COLOR=#fff` のように値の先頭に
/// 直接付いた # はコメントとみなさない (python-dotenv / docker compose と同じ)。
fn strip_comment(rest: &str) -> &str {
    // 直前の文字が空白なら、その位置
    let mut space_before = None;
    for (i, c) in rest.char_indices() {
        if c == '#' {
            if let Some(start) = space_before {
                return &rest[..start];
            }
        }
        space_before = c.is_whitespace().then_some(i);
    }
    rest
}

/// segment の中で値を閉じる引用符の位置を返す。無ければ None。
///
/// '...' はエスケープを持たないので次の ' で閉じる。
/// "..." は \ の直後を飛ばす (\" では閉じない)。バイト単位で進めるので \ の直後が
/// 多バイト文字だと文字の途中に止まるが、続きのバイトが \ や " に一致することはないので、
/// 返す位置は必ず " (つまり文字の境目) になる。
fn closing_quote(segment: &str, quote: char) -> Option<usize> {
    if quote == '\'' {
        return segment.find('\'');
    }
    let bytes = segment.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return Some(i),
            _ => i += 1,
        }
    }
    None
}

/// "..." の中のエスケープを解釈する。どの実装でも同じ意味になる
/// \n \r \t \" \\ だけを解釈し、それ以外は書かれたまま残す。
fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some(c @ ('"' | '\\')) => out.push(c),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    //! 生の値を確かめるため、ここでは scan を直接呼ぶ。

    use super::*;

    fn values(text: &str) -> Vec<(String, String)> {
        let (assignments, _) = scan(text, "t.env");
        assignments
            .into_iter()
            .map(|a| (a.key, a.value.0))
            .collect()
    }

    fn one(key: &str, value: &str) -> Vec<(String, String)> {
        vec![(key.to_owned(), value.to_owned())]
    }

    fn warnings(text: &str) -> Vec<String> {
        scan(text, "t.env").1
    }

    fn digest_of(text: &str) -> blake3::Hash {
        let (entries, _) = parse_text(text, "t.env");
        entries.get("FOO").unwrap().digest.0
    }

    #[test]
    fn value() {
        let cases = [
            ("FOO=bar", "bar"),
            ("export FOO=bar", "bar"),
            ("  FOO = bar  ", "bar"),
            ("FOO=bar # コメント", "bar"),
            ("FOO=a#b", "a#b"),
            ("FOO=#fff", "#fff"),
            ("FOO=a=b", "a=b"),
            ("FOO=${BAR}", "${BAR}"),
            ("FOO=", ""),
            ("FOO= # コメント", ""),
            ("FOO=''", ""),
            (r#"FOO="""#, ""),
            ("FOO=' bar '", " bar "),
            ("FOO='a # b' # コメント", "a # b"),
            (r##"FOO="a # b"# コメント"##, "a # b"),
            (r"FOO='a\nb'", r"a\nb"),
            (r#"FOO="a\nb""#, "a\nb"),
            (r#"FOO="a\tb""#, "a\tb"),
            (r#"FOO="say \"hi\"""#, r#"say "hi""#),
            (r#"FOO="a\\nb""#, r"a\nb"),
            (r#"FOO="a\xb""#, r"a\xb"),
            // \ の直後や値の中に多バイト文字があっても文字の途中で切らない
            (r#"FOO="\あ""#, r"\あ"),
            ("FOO=あ # コメント", "あ"),
            ("FOO='あ'", "あ"),
        ];
        for (line, expected) in cases {
            assert_eq!(values(line), one("FOO", expected), "{line}");
            assert_eq!(warnings(line), Vec::<String>::new(), "{line}");
        }
    }

    #[test]
    fn key_named_export() {
        assert_eq!(values("export=1"), one("export", "1"));
        assert_eq!(values("export =1"), one("export", "1"));
        assert_eq!(values("exportFOO=1"), one("exportFOO", "1"));
    }

    #[test]
    fn quoted_value_can_span_lines() {
        let text = "CERT=\"-----BEGIN-----\nAB==\nKEY=not-a-key\n-----END-----\"\nNEXT=1\n";
        assert_eq!(
            values(text),
            [
                (
                    "CERT".to_owned(),
                    "-----BEGIN-----\nAB==\nKEY=not-a-key\n-----END-----".to_owned()
                ),
                ("NEXT".to_owned(), "1".to_owned()),
            ]
        );
    }

    #[test]
    fn single_quoted_value_can_span_lines() {
        assert_eq!(
            values("FOO='a\nb'\nNEXT=1\n"),
            [
                ("FOO".to_owned(), "a\nb".to_owned()),
                ("NEXT".to_owned(), "1".to_owned())
            ]
        );
    }

    #[test]
    fn comments_and_blank_lines_are_skipped() {
        let text = "# FOO=1\n\n   \n  # BAR=2\nBAZ=3\n";
        assert_eq!(values(text), one("BAZ", "3"));
        assert_eq!(warnings(text), Vec::<String>::new());
    }

    #[test]
    fn crlf_and_cr_line_endings() {
        let expected = [
            ("FOO".to_owned(), "bar".to_owned()),
            ("BAZ".to_owned(), "qux".to_owned()),
        ];
        assert_eq!(values("FOO=bar\r\nBAZ='qux'\r\n"), expected);
        assert_eq!(values("FOO=bar\rBAZ='qux'\r"), expected);
    }

    #[test]
    fn unreadable_line_is_skipped_with_warning() {
        let cases = [
            (
                "just text",
                "t.env:1: KEY=VALUE の形になっていない行を飛ばした",
            ),
            (
                "1FOO=bar",
                "t.env:1: KEY=VALUE の形になっていない行を飛ばした",
            ),
            (
                "my.key=bar",
                "t.env:1: KEY=VALUE の形になっていない行を飛ばした",
            ),
            (
                "FOO='bar' baz",
                "t.env:1: FOO の値を読めないので飛ばした (閉じ引用符の後ろに余分な文字がある)",
            ),
        ];
        for (line, message) in cases {
            let text = format!("{line}\nOK=1\n");
            assert_eq!(values(&text), one("OK", "1"), "{line}");
            assert_eq!(warnings(&text), [message], "{line}");
        }
    }

    #[test]
    fn quoted_block_with_trailing_text_is_skipped_whole() {
        // 引用符の中の `B=fragment` をキーとして読み直さない
        let text = "FOO=\"a\nB=fragment\" junk\nOK=1\n";
        assert_eq!(values(text), one("OK", "1"));
        assert_eq!(
            warnings(text),
            ["t.env:1: FOO の値を読めないので飛ばした (閉じ引用符の後ろに余分な文字がある)"]
        );
    }

    #[test]
    fn unclosed_quote_skips_rest_of_file() {
        // 閉じ引用符が無ければ、後ろの行はすべて値の続きかもしれないので読まない
        for quote in ['"', '\''] {
            let text = format!("A=1\nFOO={quote}a\nB=fragment==\nOK=1\n");
            assert_eq!(values(&text), one("A", "1"), "{quote}");
            assert_eq!(
                warnings(&text),
                ["t.env:2: FOO の値を読めないので、最後の行まで飛ばした (閉じ引用符が無い)"],
                "{quote}"
            );
        }
    }

    #[test]
    fn duplicate_key_uses_last_definition() {
        let (entries, found) = parse_text("FOO=first\nBAR=1\nFOO=\n", "t.env");
        assert_eq!(
            entries.iter().map(|(key, _)| key).collect::<Vec<_>>(),
            ["FOO", "BAR"]
        );
        assert!(entries.get("FOO").unwrap().is_empty());
        assert_eq!(
            found,
            ["t.env:3: FOO が 1 行目にもあるので、後ろの定義を使う"]
        );
    }

    #[test]
    fn same_value_written_differently_is_equal() {
        for line in [
            "FOO='bar'",
            r#"FOO="bar""#,
            "export FOO=bar",
            "FOO = bar # コメント",
        ] {
            assert!(digest_of(line) == digest_of("FOO=bar"), "{line}");
        }
    }

    #[test]
    fn different_values_are_not_equal() {
        assert!(digest_of("FOO=bar") != digest_of("FOO=baz"));
    }

    #[test]
    fn empty() {
        let cases = [
            ("FOO=", true),
            ("FOO=''", true),
            (r#"FOO="""#, true),
            ("FOO= # c", true),
            ("FOO=x", false),
            ("FOO=' '", false),
        ];
        for (line, empty) in cases {
            let (entries, _) = parse_text(line, "t.env");
            assert_eq!(entries.get("FOO").unwrap().is_empty(), empty, "{line}");
        }
    }

    #[test]
    fn digest_is_keyed() {
        // 鍵なしのハッシュと一致したら、短い値は総当たりで割り出せてしまう
        assert!(digest_of("FOO=SENTINEL_parse") != blake3::hash(b"SENTINEL_parse"));
    }
}
