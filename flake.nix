{
  description = "fossrust-crm-suite dev env";
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };
  outputs = { self, nixpkgs, rust-overlay }:
  let
    system = "x86_64-linux";
    overlays = [ rust-overlay.overlays.default ];
    pkgs = import nixpkgs { inherit system overlays; };
    rust = pkgs.rust-bin.stable.latest.default;
  in {
    devShells.${system}.default = pkgs.mkShell {
      buildInputs = [
        rust
        pkgs.pkg-config
        pkgs.openssl
        pkgs.postgresql
        pkgs.cargo-watch
        pkgs.just
        pkgs.sea-orm-cli
      ];
      RUST_SRC_PATH = "${rust}/lib/rustlib/src/rust/library";
    };
  };
}
