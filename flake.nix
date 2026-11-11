{
  description = "dcli-bootc";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs = { nixpkgs, ... }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
    in {
      packages.${system}.default = pkgs.rustPlatform.buildRustPackage {
        pname = "dcli-bootc";
        version = (pkgs.lib.importTOML ./Cargo.toml).package.version;

        src = pkgs.lib.cleanSourceWith {
          src = ./.;
          filter = path: _:
            !(builtins.elem (builtins.baseNameOf path) [ "target" "dist" ".git" ]);
        };

        cargoLock.lockFile = ./Cargo.lock;

        nativeBuildInputs = [ pkgs.installShellFiles ];

        postInstall = ''
          installShellCompletion --cmd dcli-bootc \
            --bash <($out/bin/dcli-bootc completions bash) \
            --fish <($out/bin/dcli-bootc completions fish) \
            --zsh  <($out/bin/dcli-bootc completions zsh)
        '';

        meta = {
          description = "A declarative bootc image configuration manager";
          homepage = "https://gitlab.com/theblackdon/dcli-bootc";
          license = pkgs.lib.licenses.bsd0;
          mainProgram = "dcli-bootc";
          platforms = [ "x86_64-linux" ];
        };
      };
    };
}
