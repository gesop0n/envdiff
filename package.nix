{
  lib,
  rustPlatform,
}:
# envdiff: 2 つの dotenv ファイルをキー単位で比べるコマンド (値は表示しない)。
# checkPhase で cargo test を流すので、テストが落ちたらビルドごと失敗し、壊れた物はインストールされない。
let
  manifest = (lib.importTOML ./Cargo.toml).package;
in
rustPlatform.buildRustPackage {
  pname = manifest.name;
  inherit (manifest) version;

  # nix のファイルを触っただけで作り直さないよう、ビルドに要る物だけを渡す。
  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [
      ./Cargo.toml
      ./Cargo.lock
      ./src
      ./tests
    ];
  };

  # 依存クレートは Cargo.lock のチェックサムで取ってくるので、ハッシュを手で書かなくてよい。
  cargoLock.lockFile = ./Cargo.lock;

  meta = {
    inherit (manifest) description;
    mainProgram = "envdiff";
  };
}
