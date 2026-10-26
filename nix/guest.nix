{ pkgs }:
let
  init = pkgs.writeScript "pvh-smoke-init" ''
    #!/bin/sh
    set -eu
    export PATH=/bin
    mkdir -p /dev /proc /sys /tmp
    mount -t proc proc /proc
    mount -t sysfs sys /sys
    mount -t devtmpfs dev /dev
    exec >/dev/hvc0 2>&1
    echo 'PVH_SMOKE_BOOT'
    cat /proc/cmdline
    case " $(cat /proc/cmdline) " in
      *' lab.hang=1 '*) echo PVH_SMOKE_INJECT_HANG; sleep 3600 ;;
      *' lab.crash=1 '*) echo PVH_SMOKE_INJECT_CRASH; echo c > /proc/sysrq-trigger ;;
    esac
    test "$(cat /sys/hypervisor/type)" = xen || { echo PVH_SMOKE_FAIL; poweroff -f; }
    test -d /sys/bus/xen || { echo PVH_SMOKE_FAIL; poweroff -f; }
    echo 'PVH_SMOKE_ASSERT hypervisor=xen'
    echo 'PVH_SMOKE_ASSERT arithmetic=42'
    test "$((6 * 7))" = 42 || { echo PVH_SMOKE_FAIL; poweroff -f; }
    echo 'PVH_SMOKE_PASS'
    sync
    poweroff -f
    sleep 60
  '';
in pkgs.makeInitrd {
  contents = [
    { object = init; symlink = "/init"; }
    { object = "${pkgs.pkgsStatic.busybox}/bin"; symlink = "/bin"; }
  ];
}
