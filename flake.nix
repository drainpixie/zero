{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";

    hooks = {
      url = "github:cachix/git-hooks.nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    self,
    hooks,
    fenix,
    nixpkgs,
  }: let
    inherit (nixpkgs) lib;

    systems = [
      "aarch64-linux"
      "i686-linux"
      "x86_64-linux"
      "aarch64-darwin"
      "x86_64-darwin"
    ];

    forAllSystems = f:
      lib.genAttrs systems (system:
        f {
          pkgs = import nixpkgs {
            inherit system;
            overlays = [self.overlays.default];
          };

          inherit system;
        });
  in {
    overlays.default = final: prev: {
      rustToolchain = let
        pkgs = fenix.packages.${prev.stdenv.hostPlatform.system};
      in
        pkgs.combine (with pkgs.stable; [
          rustc
          cargo
          clippy
          rust-src
          pkgs.default.rustfmt
        ]);
    };

    devShells = forAllSystems ({
      pkgs,
      system,
    }: let
      check = self.checks.${system}.pre-commit-check;
    in
      pkgs.mkShell {
        inherit (check) shellHook;

        packages =
          check.enabledPackages
          ++ (builtins.attrValues {
            inherit
              (pkgs)
              rustToolchain
              cargo-edit
              cargo-watch
              rust-analyzer
              ;
          });

        env.RUST_SRC_PATH = "${pkgs.rustToolchain}/lib/rustlib/src/rust/library";
      });

    packages = forAllSystems ({pkgs, ...}: {
      default =
        (pkgs.makeRustPlatform {
          cargo = pkgs.rustToolchain;
          rustc = pkgs.rustToolchain;
        }).buildRustPackage {
          pname = "zero";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;

          OPENSSL_NO_VENDOR = 1;
        };
    });

    checks = forAllSystems ({
      system,
      pkgs,
      ...
    }: let
      hooksLib = hooks.lib.${system};
    in {
      pre-commit-check = hooksLib.run {
        src = ./.;
        package = pkgs.prek;
        hooks = {
          convco.enable = true;
          alejandra.enable = true;
          clippy = {
            enable = true;
            package = fenix.packages.${system}.stable.clippy;
          };
          rustfmt = {
            enable = true;
            package = fenix.packages.${system}.default.rustfmt;
          };
          statix = {
            enable = true;
            settings.ignore = ["/.direnv"];
          };
        };
      };
    });

    formatter = forAllSystems ({pkgs, ...}: pkgs.alejandra);
  };
}
