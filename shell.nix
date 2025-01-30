let
  rust_overlay = import (builtins.fetchTarball {
    url = "https://github.com/oxalica/rust-overlay/archive/6a3dc6ce4132bd57359214d986db376f2333c14d.tar.gz"; # master
    sha256 = "1v4h0fiifb7n3cfaqpnkkd4ynbn8ygypcwa92n5k3klyyqcy4nqq";
  });
  
  pkgs = import (fetchTarball {
    url = "https://github.com/NixOS/nixpkgs/archive/9d3ae807ebd2981d593cddd0080856873139aa40.tar.gz"; # nixos-unstable
    sha256 = "0bjqgsprq9fgl5yh58dk59xmchi4dajq3sf5i447q02dbiasjsil";
  }) { overlays = [ rust_overlay ]; };

  rust = pkgs.rust-bin.stable.latest.default.override {
    extensions = [ "rust-src" ];
  };
in
  pkgs.mkShell {
    nativeBuildInputs = [
        rust

        ### dep ###
        pkgs.pkg-config
        pkgs.dbus.dev
  ];
}
