let
  rust_overlay = import (builtins.fetchTarball {
    url = "https://github.com/oxalica/rust-overlay/archive/b4734ce867252f92cdc7d25f8cc3b7cef153e703.tar.gz"; # master
    sha256 = "1mkyvk2bl6yf95l5n604gg3zzh3m5riwcbkyd52i0gp10ni4jz2i"; # 2025-04-05T21·08+00
  });
  
  pkgs = import (fetchTarball {
    url = "https://github.com/NixOS/nixpkgs/archive/2c8d3f48d33929642c1c12cd243df4cc7d2ce434.tar.gz"; # nixos-unstable
    sha256 = "0lbn29dn647kgf3g3nzch8an3m0gn2ysrmq8l7q6lzc8lgwgif8p"; # 2025-04-05T21·08+00
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
