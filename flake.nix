{
  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    naersk.url = "github:nix-community/naersk";
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, flake-utils, naersk, nixpkgs, rust-overlay, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        toolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

        cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
        inherit (cargoToml.package) name;
        inherit (cargoToml.package) version;
        pname = (builtins.elemAt cargoToml.bin 0).name; # Get the name of the executable

        naersk' = pkgs.callPackage naersk {
          cargo = toolchain;
          rustc = toolchain;
        };

      in {
        defaultPackage = naersk'.buildPackage {
          src = ./.;

          # Use the name of the executable instead of the name of the package
          name = pname;
          inherit version;
        };

        # TODO: add some tests
        devShell = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [ 
            toolchain
            rust-analyzer
            bacon
            
            # Stuff needed for the justfile
            just
            jq
            cargo-audit
            fzf
            ripgrep
            tokei

            # Debugging
            lldb
          ];
        };
      }
    );
}
