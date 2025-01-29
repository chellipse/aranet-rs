let
  rust_overlay = import (builtins.fetchTarball {
    url = "https://github.com/oxalica/rust-overlay/archive/573c674a3ad06e8a525263185ebef336a411d1d5.tar.gz";
    sha256 = "1bp1k5qla5gwh6vc50m5pcwdfxn6g703yx1i8qrjs4l7kgh3y507";
  });
  
  pkgs = import (fetchTarball {
    url = "https://github.com/NixOS/nixpkgs/archive/d3c42f187194c26d9f0309a8ecc469d6c878ce33.tar.gz";
    sha256 = "0bmnxsn9r4qfslg4mahsl9y9719ykifbazpxxn1fqf47zbbanxkh";
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
