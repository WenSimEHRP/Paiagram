{
  description = "bevy flake";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      nixpkgs,
      rust-overlay,
      flake-utils,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        isLinux = pkgs.stdenv.hostPlatform.isLinux;

        # Define the runtime dependencies needed by Bevy (Linux only)
        runtimeLibs = pkgs.lib.optionals isLinux (with pkgs; [
          vulkan-loader
          libX11
          libXcursor
          libXi
          libXrandr
          libxkbcommon
          wayland
          libGL
          libudev-zero
          alsa-lib
          dbus
        ]);

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" ];
          targets = [
            "wasm32-unknown-unknown"
          ] ++ pkgs.lib.optionals isLinux [
            "x86_64-unknown-linux-gnu"
          ];
        };

      in
      {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "paiagram";
          version = "0.1.0";
          src = ./.;

          cargoLock.lockFile = ./Cargo.lock;
          nativeBuildInputs = [
            pkgs.pkg-config
            pkgs.openssl # TODO: remove this
            pkgs.makeWrapper
          ];
          buildInputs = runtimeLibs;
          postInstall = ''
            wrapProgram $out/bin/paiagram \
              --prefix LD_LIBRARY_PATH : "${pkgs.lib.makeLibraryPath runtimeLibs}"
          '';
        };

        devShells.default =
          with pkgs;
          mkShell (
            {
              buildInputs = [
                rustToolchain
                pkg-config
                openssl # TODO: remove this
                wasm-bindgen-cli_0_2_108
                just
                wget
                p7zip
                binaryen
                cargo-about
                gitui
              ]
              ++ runtimeLibs
              ++ lib.optionals isLinux [
                mold
                clang
                stdenv.cc.cc
              ];

              RUST_SRC_PATH = "${pkgs.rust.packages.stable.rustPlatform.rustLibSrc}";
            }
            // lib.optionalAttrs isLinux {
              LD_LIBRARY_PATH = lib.makeLibraryPath (runtimeLibs ++ [ stdenv.cc.cc ]);
            }
          );
      }
    );
}
