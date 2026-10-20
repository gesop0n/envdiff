//! フィクスチャで表せない異常系のテスト (権限、閉じたパイプ、panic)。
//!
//! tests/cli.rs と同じく、入力の値には必ず SENTINEL を含め、出力に出ていないことを確かめる。

use std::env;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_envdiff");

/// テストごとの作業ディレクトリを target/ の下に作り直し、.env と .env.backup を置く。
fn scratch(name: &str, left: &str, right: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(".env"), left).unwrap();
    fs::write(dir.join(".env.backup"), right).unwrap();
    dir
}

/// 読み取り側を閉じたパイプ。ここへ書くと Broken pipe で失敗する。
fn closed_pipe() -> Stdio {
    let (reader, writer) = io::pipe().unwrap();
    drop(reader);
    writer.into()
}

/// 終了コード・標準出力・標準エラーを返す。どちらの出力にも値が出ていないことを確かめる。
fn finish(output: Output) -> (Option<i32>, String, String) {
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(!stdout.contains("SENTINEL"), "{stdout}");
    assert!(!stderr.contains("SENTINEL"), "{stderr}");
    (output.status.code(), stdout, stderr)
}

#[test]
fn unreadable_file() {
    let dir = scratch("unreadable_file", "A=SENTINEL_a\n", "A=SENTINEL_a\n");
    let right = dir.join(".env.backup");
    fs::set_permissions(&right, fs::Permissions::from_mode(0o000)).unwrap();
    // root などは権限が無くても読めるので、この環境では確かめられない
    if fs::read(&right).is_ok() {
        eprintln!("権限が無くても読める環境なので飛ばす");
        return;
    }

    let output = Command::new(BIN)
        .args([".env", ".env.backup"])
        .current_dir(&dir)
        .output()
        .unwrap();
    let (code, stdout, stderr) = finish(output);
    assert_eq!(code, Some(2));
    assert_eq!(stdout, "");
    assert_eq!(
        stderr,
        "envdiff: .env.backup: 読めない (Permission denied)\n"
    );
}

#[test]
fn stdout_closed() {
    let dir = scratch("stdout_closed", "A=SENTINEL_a\n", "A=SENTINEL_b\n");
    let output = Command::new(BIN)
        .args([".env", ".env.backup"])
        .current_dir(&dir)
        .stdout(closed_pipe())
        .output()
        .unwrap();
    let (code, _, stderr) = finish(output);
    assert_eq!(code, Some(2));
    assert_eq!(stderr, "envdiff: 標準出力に書けない (Broken pipe)\n");
}

#[test]
fn help_to_closed_stdout() {
    let output = Command::new(BIN)
        .arg("--help")
        .stdout(closed_pipe())
        .output()
        .unwrap();
    let (code, _, stderr) = finish(output);
    assert_eq!(code, Some(2));
    assert_eq!(stderr, "envdiff: 標準出力に書けない (Broken pipe)\n");
}

#[test]
fn stderr_closed_while_warning() {
    // 1 行目が読めない行なので警告を出そうとする
    let dir = scratch(
        "stderr_closed_while_warning",
        "SENTINEL_bad_line\nA=SENTINEL_a\n",
        "A=SENTINEL_b\n",
    );
    let output = Command::new(BIN)
        .args([".env", ".env.backup"])
        .current_dir(&dir)
        .stderr(closed_pipe())
        .output()
        .unwrap();
    let (code, stdout, _) = finish(output);
    // シグナルや panic で落ちず、エラーの終了コードで終える
    assert_eq!(code, Some(2));
    assert_eq!(stdout, "");
}

/// このテストを子プロセスとして動かしていることを示す環境変数。
const PANIC_CHILD: &str = "ENVDIFF_TEST_PANIC_CHILD";

#[test]
fn panic_shows_only_fixed_message() {
    if env::var_os(PANIC_CHILD).is_some() {
        // 子プロセス側: 値の断片を含むメッセージで panic させる
        let code = envdiff::cli::guard(|| panic!("SENTINEL_in_panic"));
        std::process::exit(code.into());
    }

    // panic の表示はプロセス全体の設定を変えるので、このテストだけを子プロセスで動かす。
    // バックトレースを求められても、メッセージ・発生場所・バックトレースを出さない。
    let output = Command::new(env::current_exe().unwrap())
        .args(["--exact", "panic_shows_only_fixed_message", "--nocapture"])
        .env(PANIC_CHILD, "1")
        .env("RUST_BACKTRACE", "full")
        .output()
        .unwrap();
    let (code, _, stderr) = finish(output);
    assert_eq!(code, Some(2));
    assert_eq!(stderr, "envdiff: 内部エラー (panic)\n");
}
