{ pkgs, nixpkgs, system }:
let
      guest = import ./guest.nix { inherit pkgs; };
      dom0 = nixpkgs.lib.nixosSystem {
        inherit system;
        modules = [ ./dom0.nix ];
      };
      cfg = dom0.config;
      xen = cfg.virtualisation.xen.package;
      iso = pkgs.runCommand "xen-lab-boot.iso" {
        nativeBuildInputs = [ pkgs.grub2 pkgs.xorriso ];
      } ''
        mkdir -p image/boot/grub
        cp ${xen.boot}/${xen.multiboot} image/boot/xen.gz
        cp ${cfg.system.build.kernel}/bzImage image/boot/linux
        cp ${cfg.system.build.netbootRamdisk}/initrd image/boot/initrd
        cat > image/boot/grub/grub.cfg <<CFG
        serial --unit=0 --speed=115200
        terminal_input serial
        terminal_output serial
        set timeout=0
        menuentry 'Xen development laboratory' {
          multiboot /boot/xen.gz ${nixpkgs.lib.concatStringsSep " " cfg.virtualisation.xen.boot.params}
          module /boot/linux init=${cfg.system.build.toplevel}/init console=hvc0 earlyprintk=xen panic=-1
          module /boot/initrd
        }
        CFG
        grub-mkrescue -o $out image
      '';
      lab = pkgs.linkFarm "xen-lab" [
        { name = "boot.iso"; path = iso; }
        { name = "guest-kernel"; path = "${pkgs.linuxPackages.kernel}/bzImage"; }
        { name = "guest-initrd"; path = "${guest}/initrd"; }
        { name = "qemu"; path = "${pkgs.qemu_kvm}/bin/qemu-system-x86_64"; }
        { name = "versions.json"; path = pkgs.writeText "lab-versions.json" (builtins.toJSON {
          nixpkgs = nixpkgs.rev;
          xen = xen.version;
          linux = pkgs.linuxPackages.kernel.version;
          qemu = pkgs.qemu_kvm.version;
          busybox = pkgs.pkgsStatic.busybox.version;
          rust = pkgs.rustc.version;
        }); }
      ];
in { inherit lab dom0 iso guest; }
