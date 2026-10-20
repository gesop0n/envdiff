//! 2 つの dotenv ファイル (.env と .env.local など) をキー単位で比べる。
//!
//! 普通の diff だと並び順・コメント・引用符の書き方の違いまで差分に出るうえ、
//! 秘密情報の値がそのまま画面 (や AI エージェントの会話ログ) に残る。
//! envdiff はキーごとに「片方にしか無い / 値が違う / 一致」と「空かどうか」だけを示し、
//! 値そのものは決して表示しない。表示させるオプションも作らない。
//!
//! 値を出さないことは「表示しないよう気を付ける」ではなく、作りで保証する:
//!   - 生の値に触れるのは parse モジュールの中だけ。外へ渡す [`Entry`] が持つのは
//!     「空かどうか」と、実行ごとのランダムな鍵で取ったハッシュだけで、Debug も Display も
//!     実装しない。表示しようとするコードはコンパイルが通らない。
//!   - 警告・エラーのメッセージに行の中身を入れない (ファイル名・行番号・キー名だけ)。
//!     比較結果の注記 ([`Change::note`]) は `&'static str` なので、固定の文言しか入らない。
//!   - panic のメッセージには値の断片が入りうるので、決まった文言だけを出す ([`cli::guard`])。
//!
//! ただしキー名は表示する。引用符で囲まずに複数行の値を書いた壊れたファイルでは、
//! 値の断片が `XXXX=` の形 (base64 のパディングなど) になってキーと区別できず、
//! キー名として表示されうる。これは dotenv の書式上どうにもならない。
//! 引用符で囲んだ値なら、閉じ引用符が無かったり後ろに余分な文字があったりしても、
//! 値の途中の行をキーとして読み直さない (その代わり、閉じ引用符が無ければ最後まで読まない)。

pub mod cli;
mod diff;
mod parse;

use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

pub use diff::{Change, Mark, compare, render};
pub use parse::{Entries, Entry, parse_text};

/// 利用者に見せて終了するエラー。メッセージに値の中身を入れないこと。
#[derive(Debug)]
pub struct Error(String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

/// dotenv ファイルを読み、キーごとの Entry と警告を返す。
pub fn read_env(path: &Path) -> Result<(Entries, Vec<String>), Error> {
    let shown = path.display();
    let bytes = fs::read(path).map_err(|e| Error(format!("{shown}: 読めない ({})", reason(&e))))?;
    // 先頭に BOM があれば落とす (Windows のエディタで保存したファイルなど)
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
    let text =
        std::str::from_utf8(bytes).map_err(|_| Error(format!("{shown}: UTF-8 として読めない")))?;
    Ok(parse_text(text, &shown.to_string()))
}

/// io::Error の説明を返す。OS のエラーなら末尾の ` (os error 2)` を落とす。
pub(crate) fn reason(e: &io::Error) -> String {
    let text = e.to_string();
    match e.raw_os_error() {
        Some(code) => text
            .strip_suffix(&format!(" (os error {code})"))
            .map_or(text.clone(), str::to_owned),
        None => text,
    }
}
