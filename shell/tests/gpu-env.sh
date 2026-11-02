#!/bin/sh
# rea_gpu_env against fake sysfs trees and ICD dirs: sh shell/tests/gpu-env.sh
# (merged from the CMR3/CMR4/OR/SCR prime tests and SRR's ICD-pin test).
. "$(dirname "$(readlink -f "$0")")/common.sh"
REA_APP=test-launcher
load_lib
# NVIDIA's OpenGL library lookup, faked (SCR's test hook).
fake_nv_lib() { [ -n "${NV_LIB:-}" ]; }
REA_NV_LIB_CHECK=fake_nv_lib

gpu() { # tree card vendor driver boot_vga [vram]
  d=$T/$1/class/drm/$2/device
  mkdir -p "$d" "$T/$1/drivers/$4" "$T/$1/class/drm/$2-DP-1"
  echo "$3" > "$d/vendor"; echo "$5" > "$d/boot_vga"
  ln -s "$T/$1/drivers/$4" "$d/driver"
  [ -n "${6:-}" ] && echo "$6" > "$d/mem_info_vram_total"
  return 0
}
prime() { # name tree expected [NV_LIB]
  check "$1" "$(REA_SYSFS=$T/$2 NV_LIB=${4:-} rea_gpu_env)" "$3"
}
NV="__NV_PRIME_RENDER_OFFLOAD=1 __GLX_VENDOR_LIBRARY_NAME=nvidia __VK_LAYER_NV_optimus=NVIDIA_only"

echo "# prime rule (REA_GPU_ICD_PIN=0)"
gpu nv card0 0x10de nvidia 1
prime "one NVIDIA card driving the screen" nv "$NV" yes
gpu amdnv card0 0x1002 amdgpu 0 2147483648; gpu amdnv card1 0x10de nvidia 1
prime "AMD iGPU + NVIDIA on the screen" amdnv "$NV" yes
gpu laptop card0 0x8086 i915 1; gpu laptop card1 0x10de nvidia 0
prime "Intel laptop screen + NVIDIA dGPU" laptop "$NV" yes
prime "NVIDIA without its OpenGL library: Mesa rule" laptop "DRI_PRIME=1"
gpu inamd card0 0x8086 i915 1; gpu inamd card1 0x1002 amdgpu 0 8589934592
prime "Intel iGPU + AMD dGPU" inamd "DRI_PRIME=1"
gpu apu card0 0x1002 amdgpu 1 536870912; gpu apu card1 0x1002 amdgpu 0 8589934592
prime "AMD APU on the screen + AMD dGPU" apu "DRI_PRIME=1"
gpu dgpu card0 0x1002 amdgpu 1 8589934592; gpu dgpu card1 0x1002 amdgpu 0 536870912
prime "AMD dGPU on the screen + AMD APU" dgpu ""
gpu intel card0 0x8086 i915 1
prime "Intel only" intel ""
gpu amd card0 0x1002 amdgpu 1 8589934592
prime "AMD only" amd ""
prime "no GPUs listed" none ""
DRI_PRIME=0 prime "the player's DRI_PRIME wins" inamd ""
__NV_PRIME_RENDER_OFFLOAD=0 prime "the player's NVIDIA offload choice wins" laptop "" yes
echo garbage > "$T/apu/class/drm/card0/device/mem_info_vram_total"
prime "unreadable VRAM counts as none" apu "DRI_PRIME=1"

# Without the hook, the library lookup goes through the wrapper (the
# container that has the driver).
unset REA_NV_LIB_CHECK
printf '#!/bin/sh\necho "$*" > "$T/nvq"; [ -e "$T/has-nv" ]\n' > "$T/nv-run"; chmod +x "$T/nv-run"
REA_RUN=$T/nv-run
: > "$T/has-nv"
prime "NVIDIA library looked up through REA_RUN" laptop "$NV"
contains "the lookup asks ldconfig for libGLX_nvidia" "$(cat "$T/nvq")" "libGLX_nvidia.so.0"
rm "$T/has-nv"
prime "no library inside the wrapper: Mesa rule" laptop "DRI_PRIME=1"
REA_RUN= REA_NV_LIB_CHECK=fake_nv_lib

echo "# NVIDIA Vulkan ICD pin (REA_GPU_ICD_PIN=1, SRR)"
REA_GPU_ICD_PIN=1
mkdir -p "$T/nvpci/bus/pci/devices/0000:01:00.0" "$T/nvmod/module/nvidia" \
  "$T/amdpci/bus/pci/devices/0000:0c:00.0" "$T/empty" "$T/hybrid"
echo 0x10de > "$T/nvpci/bus/pci/devices/0000:01:00.0/vendor"
echo 0x1002 > "$T/amdpci/bus/pci/devices/0000:0c:00.0/vendor"
echo '{}' > "$T/hybrid/nvidia_icd.json"; echo '{}' > "$T/hybrid/radeon_icd.json"
pin() { # name icd-dirs sysfs expected
  check "$1" "$(REA_ICD_DIRS=$2 REA_SYSFS=$3 rea_gpu_env)" "$4"
}
want="VK_DRIVER_FILES=$T/hybrid/nvidia_icd.json VK_ICD_FILENAMES=$T/hybrid/nvidia_icd.json"
pin "hybrid host with an NVIDIA PCI device pins its ICD" "$T/hybrid" "$T/nvpci" "$want"
pin "a loaded nvidia module counts as a device" "$T/hybrid" "$T/nvmod" "$want"
pin "a leftover NVIDIA ICD without an NVIDIA device is not pinned" "$T/hybrid" "$T/amdpci" ""
mkdir -p "$T/etc" "$T/usr"
echo '{}' > "$T/etc/nvidia_icd.json"; echo '{}' > "$T/usr/intel_icd.x86_64.json"
pin "/etc and /usr/share ICD dirs both count" "$T/etc $T/usr" "$T/nvpci" \
  "VK_DRIVER_FILES=$T/etc/nvidia_icd.json VK_ICD_FILENAMES=$T/etc/nvidia_icd.json"
echo '{}' > "$T/usr/nvidia_icd.i686.json"
got=$(REA_ICD_DIRS="$T/etc $T/usr" REA_SYSFS=$T/nvpci rea_gpu_env)
contains "every NVIDIA json is listed" "$got" "VK_DRIVER_FILES=$T/etc/nvidia_icd.json:$T/usr/nvidia_icd.i686.json "
for preset in VK_ICD_FILENAMES VK_DRIVER_FILES DRI_PRIME __NV_PRIME_RENDER_OFFLOAD; do
  got=$(env "$preset=already" REA_ICD_DIRS="$T/hybrid" REA_SYSFS="$T/nvpci" REA_APP=t \
    REA_GPU_ICD_PIN=1 sh -c '. "$1"; rea_gpu_env' sh "$LIB")
  check "the caller's $preset wins" "$got" ""
done
mkdir -p "$T/single"; echo '{}' > "$T/single/intel_icd.json"
pin "a single-vendor host is left alone" "$T/single" "$T/empty" ""
mkdir -p "$T/prime-icd"; echo '{}' > "$T/prime-icd/radeon_icd.json"
pin "no NVIDIA ICD: Intel iGPU + AMD dGPU falls back to DRI_PRIME" "$T/prime-icd" "$T/inamd" "DRI_PRIME=1"
pin "no NVIDIA ICD: AMD APU + bigger dGPU by VRAM" "$T/prime-icd" "$T/apu" "DRI_PRIME=1"
NV_LIB=yes pin "pin mode never uses NVIDIA's OpenGL offload (Mesa rule)" "$T/prime-icd" "$T/laptop" "DRI_PRIME=1"

echo "# rea_gpu_init"
REA_GPU_ICD_PIN=0
unset REA_GPU
REA_SYSFS=$T/inamd rea_gpu_init > "$T/out"
check "REA_GPU computed once" "$REA_GPU" "DRI_PRIME=1"
contains "and reported" "$(cat "$T/out")" "GPU: DRI_PRIME=1"
REA_SYSFS=$T/intel rea_gpu_init > /dev/null
check "a preset REA_GPU is kept" "$REA_GPU" "DRI_PRIME=1"
finish
