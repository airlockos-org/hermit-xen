# Third-party provenance and notices

Original hermit-xen code is licensed under MIT OR Apache-2.0. Upstream code and
submodules retain their own notices and license choices; this repository does
not relicense upstream projects or the binaries built by the laboratory.

- Hermit kernel and hermit-rs: MIT OR Apache-2.0; kernel pinned by Git submodule, wrapper by flake.lock and Cargo locks.
- rust-vmm/xen: MIT OR Apache-2.0. The Linux event-device adapter follows its
  UnixEventChannelInterface and ioctl layouts. Copyright (c) 2016–2017 Doug
  Goldstein; individual source notices (including Mathieu Poirier's) remain in
  upstream files and submodules. Its MIT notice is reproduced below.
- Mini-OS arch/x86/time.c: MIT, consulted for PV clock scaling and event semantics.
  Copyright 2003 Rolf Neugebauer, Intel Research Cambridge; 2002–2003 Keir Fraser,
  University of Cambridge; 2005 Grzegorz Milos, Intel Research Cambridge;
  2006 Robert Kaiser, FH Wiesbaden. Its permission notice is reproduced below.
- Mirage Xen event bindings: ISC, consulted for event unmask/race handling.
  Copyright (c) 2020 Martin Lucina <martin@lucina.net>. Notice below.
- Xen/Linux public ABI definitions and hypervisor/driver implementations were
  consulted to implement hypercalls and ioctls. No Linux or Xen implementation
  source is vendored here. Generated bindings are provided by pinned rust-vmm/xen.

The laboratory downloads/builds Xen, Linux, QEMU, NixOS and other dependencies
under their respective licenses (including GPL licenses). The project's dual
license does not apply to those separately built artifacts. Cargo and Nix locks
identify the dependency inputs; upstream license files remain in their source
and package outputs. The vendor Git links refer to upstream-history-preserving forks; built VM images
are not committed. See [fork provenance](docs/contributing.md#provenance).

## rust-vmm/xen MIT notice

Copyright (c) 2016-2017 Doug Goldstein

Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.

## Mini-OS permission notice

Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.

## Mirage Xen ISC notice

Permission to use, copy, modify, and distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
