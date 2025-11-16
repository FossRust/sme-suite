{
  description = "Dev environment for the SME suite";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ rust-overlay.overlays.default ];
        pkgs = import nixpkgs {
          inherit system;
          overlays = overlays;
        };
        rustToolchain = pkgs.rust-bin.stable.latest.default;
      in {
        devShells.default = pkgs.mkShell {
          buildInputs = [
            rustToolchain
            pkgs.cargo-watch
            pkgs.just
            pkgs.sea-orm-cli
            pkgs.sqlx-cli
            pkgs.wasm-pack
            pkgs.nodejs_22
            pkgs.openssl
            pkgs.pkg-config
            pkgs.protobuf
          ];
          shellHook = ''
            export DATABASE_URL="postgres://postgres:postgres@localhost:5432/sme_suite"
            export RUST_LOG="info"
            echo "suite-shell ready: $(rustc --version)"
          '';
        };
      });
}
