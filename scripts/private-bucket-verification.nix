# Reproducible native prerequisites for private bucket verification.
# Run from the utilities workspace with all nested utility checkouts present.
{ pkgs ? import <nixpkgs> {} }:
pkgs.mkShell {
  nativeBuildInputs = with pkgs; [ pkg-config nodejs git openssh util-linux python3 age ];
  buildInputs = [ pkgs.openssl ];
  OPENSSL_INCLUDE_DIR = "${pkgs.openssl.dev}/include";
  OPENSSL_LIB_DIR = "${pkgs.openssl.out}/lib";
  shellHook = ''
    # Use the installed rustup toolchain (Rust >= 1.89 required by the crate),
    # not the older compiler in some operator Nix channels. No global change.
    export PATH="$HOME/.cargo/bin:$HOME/.bun/bin:$HOME/.local/bin:$PATH"
    unset OPENSSL_DIR OPENSSL_STATIC
    export PKG_CONFIG_PATH="${pkgs.openssl.dev}/lib/pkgconfig:$PKG_CONFIG_PATH"
    export RUST_TEST_THREADS=1
  '';
}
