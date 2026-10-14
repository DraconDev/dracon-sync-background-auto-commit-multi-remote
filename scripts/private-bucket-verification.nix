# Reproducible native prerequisites for private bucket verification.
# Run from the utilities workspace with all nested utility checkouts present.
{ pkgs ? import <nixpkgs> {} }:
pkgs.mkShell {
  nativeBuildInputs = with pkgs; [ cargo rustc pkg-config nodejs bun git util-linux ];
  buildInputs = [ pkgs.openssl ];
  OPENSSL_INCLUDE_DIR = "${pkgs.openssl.dev}/include";
  OPENSSL_LIB_DIR = "${pkgs.openssl.out}/lib";
  shellHook = ''
    unset OPENSSL_DIR OPENSSL_STATIC
    export PKG_CONFIG_PATH="${pkgs.openssl.dev}/lib/pkgconfig:$PKG_CONFIG_PATH"
    export RUST_TEST_THREADS=1
  '';
}
