//! コマンドとしての振る舞いのテスト。
//!
//! tests/fixtures/<分類>/<ケース名>/ のディレクトリ 1 つが 1 ケース。分類は次の 3 つ:
//!
//!   normal       正常系。正しい dotenv を比べる
//!   semi_normal  準正常系。仕様として想定している不正な入力や境界の入力
//!                (警告を出して続けるもの、引数の誤りで終了コード 2 になるもの)
//!   abnormal     異常系。環境が原因の失敗 (ファイルが無い、ディレクトリ、UTF-8 でない)
//!
//! フィクスチャで表せない異常系 (権限、閉じたパイプ、panic) は tests/abnormal.rs にある。
//!
//! ケースのディレクトリの中で envdiff を実行し、結果を期待値のファイルと比べる:
//!
//!   args             渡す引数。1 行に 1 つ
//!   expected.status  終了コード
//!   expected.stdout  標準出力。ファイルが無ければ空を期待する
//!   expected.stderr  標準エラー。ファイルが無ければ空を期待する
//!
//! ほかのファイル (.env など) は入力。入力の値には必ず SENTINEL を含めること。どのケースでも
//! 「標準出力にも標準エラーにも SENTINEL が出ていないこと」を確かめる。
//!
//! 期待値は `ENVDIFF_BLESS=1 cargo test` で今の出力から作り直せる。
//! 作り直したら、git diff で変わった所を必ず読むこと。

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

#[test]
fn fixtures() {
    // 0 や空文字で作り直しを止めたつもりの設定を、作り直しと取り違えないよう "1" だけを認める
    let bless = env::var("ENVDIFF_BLESS").is_ok_and(|v| v == "1");
    let mut cases = Vec::new();
    collect_cases(Path::new(FIXTURES), &mut cases);
    cases.sort();
    assert!(!cases.is_empty(), "tests/fixtures にケースが無い");

    let failures: Vec<String> = cases.iter().flat_map(|case| check(case, bless)).collect();
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// args を持つディレクトリをケースとして集める。
fn collect_cases(dir: &Path, cases: &mut Vec<PathBuf>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|e| panic!("{} を読めない ({e})", dir.display()));
    for entry in entries {
        let path = entry.unwrap().path();
        if !path.is_dir() {
            continue;
        }
        if path.join("args").is_file() {
            cases.push(path);
        } else {
            collect_cases(&path, cases);
        }
    }
}

/// 1 ケースを実行し、期待と違った点を返す。bless なら期待値を今の出力で書き換える。
fn check(case: &Path, bless: bool) -> Vec<String> {
    let name = case.strip_prefix(FIXTURES).unwrap().display();
    let args = fs::read_to_string(case.join("args"))
        .unwrap_or_else(|e| panic!("{name}: args を読めない ({e})"));
    let output = Command::new(env!("CARGO_BIN_EXE_envdiff"))
        .args(args.lines())
        .current_dir(case)
        // 手元の環境変数で色が付いたり消えたりしないようにする
        .env_remove("NO_COLOR")
        .env_remove("CLICOLOR")
        .env_remove("CLICOLOR_FORCE")
        .output()
        .expect("envdiff を実行できない");
    let status = match output.status.code() {
        Some(code) => format!("{code}\n"),
        None => "シグナルで終了した\n".to_owned(),
    };
    let actual = [
        ("expected.status", status),
        (
            "expected.stdout",
            String::from_utf8_lossy(&output.stdout).into_owned(),
        ),
        (
            "expected.stderr",
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ),
    ];

    let mut failures = Vec::new();
    for (file, text) in &actual {
        let path = case.join(file);
        // 値が出ている出力は、作り直すときも期待値にしない
        if text.contains("SENTINEL") {
            failures.push(format!("{name}: 出力に値が出ている ({file})\n{text}"));
        } else if bless {
            bless_file(&path, text);
        } else {
            let expected = read_or_empty(&path);
            if expected != *text {
                failures.push(format!(
                    "{name}: {file} と違う\n--- 期待\n{expected}--- 実際\n{text}"
                ));
            }
        }
    }
    failures
}

/// 期待値のファイルを読む。無ければ空。
fn read_or_empty(path: &Path) -> String {
    match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => panic!("{} を読めない ({e})", path.display()),
    }
}

/// 期待値のファイルを書き換える。空ならファイルを置かない。
fn bless_file(path: &Path, text: &str) {
    let result = if text.is_empty() {
        fs::remove_file(path).or_else(|e| match e.kind() {
            io::ErrorKind::NotFound => Ok(()),
            _ => Err(e),
        })
    } else {
        fs::write(path, text)
    };
    result.unwrap_or_else(|e| panic!("{} に書けない ({e})", path.display()));
}
