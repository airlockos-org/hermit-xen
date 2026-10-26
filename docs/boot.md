# Native PVH boot

Xen loads the application's ELF through Hermit's upstream PVH entry note. The
kernel fork selects a Xen platform path, uses native hypercalls and Xen console,
initializes shared-info, and reports poweroff or crash through Xen. This is not
QEMU's Xen ABI emulation and requires no separate guest loader.

The laboratory boots GRUB → real Xen → NixOS dom0 under QEMU/KVM, then creates a
single-vCPU PVH domain. Console output is captured by xenconsoled without a client
attached. The first boot milestone was compared against upstream PVH under direct
QEMU; release regression tests require actual Xen.

The bootstrap identity map covers Xen's reserved console frame below 4 GiB.
Shared-info selects the 64-bit ABI through the HVM callback parameter; native
vmcall/vmmcall needs no hypercall transfer page. The IPC integration installs
an event callback vector and PV timers, replacing the initial virtual-APIC timer
path for Xen. See [IPC architecture](xen-ipc.md).

The maintained source lives in `vendor/hermit-kernel` and `vendor/rust-vmm-xen`:

```sh
mkdir -p .lab/build
nix build .#hermit-sources -o .lab/build/hermit-sources
nix build .#hermit-kernel -o .lab/build/hermit-kernel
nix develop .#hermit
```

Edit the submodules directly; see [contributing](contributing.md) for commits,
pins and upstream submissions.
The laboratory retains debug ELF images, hashes and frame pointers. On failures,
inspect guest console, Xen logs and `guest-registers.txt`; `guest-rip-symbol.txt`
uses the retained image so later rebuilds cannot change the address interpretation.

The permissive upstream bootstrap mappings, output-only console and fallback
entropy source remain limitations. This work does not establish cryptographic
entropy, SMP, migration, suspend/resume or a live domU debugger.
