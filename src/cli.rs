//! コマンドとしての入口。引数を読み、2 つのファイルを比べて表示する。

use std::env;
use std::io::{self, IsTerminal, Write};
use std::panic::{self, UnwindSafe};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

use crate::{Error, Mark, compare, read_env, reason, render};

/// --help に出す解釈のしかた。dotenv には標準仕様が無く、Node の dotenv /
/// python-dotenv / docker compose で細部が違うので、どれでもほぼ同じに読める範囲に絞る。
const RULES: &str = r#"解釈のしかた:
  KEY=VALUE        先頭の `export ` と `=` 前後の空白は許す
                   キーは英数字と _ だけ (先頭は数字以外)
  # ...            `#` で始まる行はコメント
  KEY=value # ...  引用符なしの値は「空白 + #」以降がコメント。前後の空白は落とす
  KEY='...'        書いたまま。複数行にまたがってよい
  KEY="..."        \n \r \t \" \\ を解釈する。複数行にまたがってよい
  ${VAR}           展開しない (書かれたまま比べる)
  同じキーが複数回あれば後ろの定義を使い、警告を出す。読めない行は警告を出して飛ばす。
  閉じ引用符が無い値があれば、後ろの行は値の続きかもしれないので、最後まで読まない。

終了コード: 0 差分なし / 1 差分あり / 2 エラー"#;

/// 2 つの dotenv ファイルをキー単位で比べる。値は表示しない。
#[derive(Parser)]
// args_override_self: `-a -a` のように同じフラグが重なっても誤りにしない
#[command(name = "envdiff", after_help = RULES, args_override_self = true)]
struct Args {
    /// 比べる元 (例: .env)
    #[arg(value_name = "FILE_A")]
    left: PathBuf,

    /// 比べる先 (例: .env.backup)
    #[arg(value_name = "FILE_B")]
    right: PathBuf,

    /// 一致したキーも表示する
    #[arg(short, long)]
    all: bool,

    /// キーの有無だけ比べる (値の違いは無視する)
    #[arg(short, long)]
    keys_only: bool,
}

/// 終了コードは 0 差分なし / 1 差分あり / 2 エラー (引数の誤りを含む)。
pub fn main() -> ExitCode {
    ExitCode::from(guard(|| {
        let args = match Args::try_parse() {
            Ok(args) => args,
            Err(e) => return show_clap(&e),
        };
        match run(&args) {
            Ok(differs) => u8::from(differs),
            Err(e) => {
                // 標準エラーにも書けないときは、終了コードだけで伝える
                let _ = writeln!(io::stderr(), "envdiff: {e}");
                2
            }
        }
    }))
}

/// clap が作った --help の表示や引数の誤りを出し、終了コードを返す。
///
/// Parser::parse は --help を標準出力に書けなくても 0 で終えるので、自前で出す。
fn show_clap(e: &clap::Error) -> u8 {
    if let Err(write) = e.print() {
        if !e.use_stderr() {
            let _ = writeln!(
                io::stderr(),
                "envdiff: 標準出力に書けない ({})",
                reason(&write)
            );
        }
        return 2;
    }
    // --help は 0、引数の誤りは 2
    u8::try_from(e.exit_code()).unwrap_or(2)
}

/// 比べて表示する。差分があれば true を返す。
fn run(args: &Args) -> Result<bool, Error> {
    let (left, left_warnings) = read_env(&args.left)?;
    let (right, right_warnings) = read_env(&args.right)?;
    let mut stderr = io::stderr().lock();
    for warning in left_warnings.iter().chain(&right_warnings) {
        writeln!(stderr, "envdiff: 警告: {warning}")
            .map_err(|e| Error(format!("標準エラーに書けない ({})", reason(&e))))?;
    }

    let changes = compare(&left, &right, args.keys_only);
    let color = io::stdout().is_terminal() && env::var_os("NO_COLOR").is_none_or(|v| v.is_empty());
    let text = render(
        &args.left.display().to_string(),
        &args.right.display().to_string(),
        &changes,
        args.all,
        color,
    );
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
        .map_err(|e| Error(format!("標準出力に書けない ({})", reason(&e))))?;
    Ok(changes.iter().any(|c| c.mark != Mark::Same))
}

/// f を実行し、panic したら決まった文言だけを出して終了コード 2 を返す。
///
/// panic のメッセージには値の断片が入りうる (たとえば文字の途中で &str を切り出すと、
/// 標準ライブラリは文字列の中身ごと報告する)。なのでメッセージも発生場所も出さない。
pub fn guard(f: impl FnOnce() -> u8 + UnwindSafe) -> u8 {
    panic::set_hook(Box::new(|_| {
        // stderr に書けなくても、ここで panic し直さない
        let _ = writeln!(io::stderr(), "envdiff: 内部エラー (panic)");
    }));
    panic::catch_unwind(f).unwrap_or(2)
}
