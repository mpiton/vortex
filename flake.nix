{
  description = "Vortex — Desktop download manager (Tauri 2 + Rust + React)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" "clippy" "rustfmt" ];
          targets = [ "wasm32-wasip1" "wasm32-unknown-unknown" ];
        };

        # Linux-specific libraries for Tauri/WebKitGTK
        linuxLibs = with pkgs; [
          gtk3
          webkitgtk_4_1
          libsoup_3
          glib-networking
          librsvg
          libayatana-appindicator
          cairo
          pango
          gdk-pixbuf
          atk
        ];

        linuxBuildInputs = with pkgs;
          (if pkgs.stdenv.isLinux then linuxLibs else []);

        linuxNativeBuildInputs = pkgs.lib.optionals pkgs.stdenv.isLinux [
          pkgs.pkg-config
        ];

      in
      {
        devShells.default = pkgs.mkShell {
          nativeBuildInputs = linuxNativeBuildInputs;

          buildInputs = [
            rustToolchain
            pkgs.nodejs_22
            pkgs.cargo-llvm-cov
            pkgs.openssl
            pkgs.pkg-config
          ] ++ linuxBuildInputs;

          shellHook = ''
            echo "Vortex dev environment loaded"
            echo "  Rust: $(rustc --version)"
            echo "  Node: $(node --version)"
            echo "  Cargo: $(cargo --version)"
          '' + pkgs.lib.optionalString pkgs.stdenv.isLinux ''
            export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath linuxLibs}:$LD_LIBRARY_PATH"
            export GIO_MODULE_PATH="${pkgs.glib-networking}/lib/gio/modules"
          '';

          RUST_BACKTRACE = 1;
        };
      }
    );
}
