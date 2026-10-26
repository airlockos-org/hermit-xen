{ config, lib, pkgs, modulesPath, ... }:
{
  imports = [ "${modulesPath}/installer/netboot/netboot.nix" ];
  system.stateVersion = "26.05";
  networking.hostName = "xen-lab-dom0";
  networking.useDHCP = false;
  networking.enableIPv6 = false;
  documentation.enable = false;
  documentation.nixos.enable = false;
  programs.command-not-found.enable = false;
  nix.enable = false;
  services.udisks2.enable = false;
  boot.loader.systemd-boot.enable = true; # Xen module's supported boot policy; ISO uses GRUB multiboot.
  boot.initrd.systemd.enable = true;
  boot.kernelPackages = pkgs.linuxPackages;
  boot.initrd.availableKernelModules = [ "virtio_pci" "virtio_console" "9p" "9pnet_virtio" ];
  boot.kernelModules = [ "virtio_console" "9pnet_virtio" "xen_gntalloc" "xen_gntdev" "xen_evtchn" ];
  virtualisation.xen = {
    enable = true;
    boot.params = [ "console=com1" "com1=115200,8n1" "noreboot" "iommu=off" ];
    dom0Resources = { memory = 4096; maxMemory = 4096; maxVCPUs = 2; };
    trace = true;
  };
  fileSystems."/lab" = {
    device = "artifacts";
    fsType = "9p";
    options = [ "trans=virtio" "version=9p2000.L" "msize=262144" "cache=none" "sync" ];
  };
  fileSystems."/inputs" = {
    device = "inputs";
    fsType = "9p";
    options = [ "trans=virtio" "version=9p2000.L" "msize=262144" "ro" ];
  };
  netboot.squashfsCompression = "zstd -Xcompression-level 3";
  services.journald.extraConfig = "ForwardToConsole=yes\nTTYPath=/dev/hvc0\nMaxLevelConsole=info";
  # Xen owns console capture, including output from domains without an attached client.
  systemd.services.xenconsoled = {
    requires = [ "lab.mount" ];
    after = [ "lab.mount" ];
    environment.XENCONSOLED_LOG_DIR = "/lab/xen-console";
  };
  systemd.services.lab-journal = {
    wantedBy = [ "multi-user.target" ];
    requires = [ "lab.mount" ];
    after = [ "lab.mount" "systemd-journald.service" ];
    serviceConfig = {
      ExecStart = "${pkgs.systemd}/bin/journalctl -b -f --no-pager -o short-monotonic";
      StandardOutput = "append:/lab/journal-live.log";
      Restart = "on-failure";
    };
  };
  systemd.services.lab-rpc = {
    wantedBy = [ "multi-user.target" ];
    after = [ "xenstored.service" "xenconsoled.service" "xen-init-dom0.service" "local-fs.target" ];
    requires = [ "lab.mount" "inputs.mount" ];
    path = [ pkgs.coreutils pkgs.util-linux pkgs.procps config.virtualisation.xen.package
      "${config.virtualisation.xen.package}/libexec/xen" ];
    serviceConfig = {
      ExecStart = "${pkgs.python3}/bin/python3 ${./rpc.py}";
      Restart = "on-failure";
      RestartSec = 1;
      StandardOutput = "journal+console";
    };
  };
}
