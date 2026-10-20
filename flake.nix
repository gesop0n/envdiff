{
  description = "envdiff 環境構築";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      nixpkgs,
      flake-utils,
      rust-overlay,
      ...
    }:
    # overlay は system に依存しないので eachSystem の外に出す。
    # dotfiles などからは pkgs.envdiff として使う。
    {
      overlays.default = final: _prev: {
        envdiff = final.callPackage ./package.nix { };
      };
    }
    # nixpkgs 26.11 が x86_64-darwin のサポートを打ち切ったため、eachDefaultSystem は
    # 使えない。ビルドできる system だけを明示的に並べる。
    //
      flake-utils.lib.eachSystem
        [
          "aarch64-darwin"
          "aarch64-linux"
          "x86_64-linux"
        ]
        (
          system:
          let
            pkgs = import nixpkgs {
              inherit system;
              overlays = [ rust-overlay.overlays.default ];
            };
            # 開発環境とビルドで同じ Rust を使う。バージョンは rust-toolchain.toml で決める。
            rust = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
            rustPlatform = pkgs.makeRustPlatform {
              cargo = rust;
              rustc = rust;
            };
            envdiff = pkgs.callPackage ./package.nix { inherit rustPlatform; };
          in
          {
            formatter = pkgs.nixfmt-tree;

            # ビルドの途中で cargo test も流れる。
            packages = {
              inherit envdiff;
              default = envdiff;
            };

            checks.envdiff = envdiff;

            devShells.default = pkgs.mkShell {
              packages = [ rust ];
            };
          }
        );
}
