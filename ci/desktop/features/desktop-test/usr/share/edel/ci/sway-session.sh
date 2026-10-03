#!/bin/sh
# Test only (roadmap M4.1): CI's session, started by greetd as user ci.
# QEMU's virtio-vga has no 3D, so Mesa draws with llvmpipe into the GPU's
# buffers (LIBGL_ALWAYS_SOFTWARE), wlroots is told to accept that, and
# sway uses GLES only, the one renderer our compositor will have.
# virtio-gpu copies a buffer to QEMU when it is flipped, and nothing makes
# the flip wait for llvmpipe's threads, so screenshots showed half-drawn
# frames; with LP_NUM_THREADS=0 llvmpipe finishes drawing before the
# flip. The log goes to /tmp, which outlives the session's runtime
# directory.
export LIBGL_ALWAYS_SOFTWARE=1 LP_NUM_THREADS=0 WLR_RENDERER=gles2 WLR_RENDERER_ALLOW_SOFTWARE=1
exec sway --config /usr/share/edel/ci/sway.conf >/tmp/sway-ci.log 2>&1
