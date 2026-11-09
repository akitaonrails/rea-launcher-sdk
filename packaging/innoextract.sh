# rea-launcher-sdk packaging/innoextract.sh: bundles innoextract 1.9's
# Windows build, which a launcher uses to unpack a game's Inno Setup
# installer (rea_launcher::installer, docs/playbooks/installers.md).
# POSIX sh library; source it after pins.sh, don't run it.
#
# The launcher's pin file (packaging/third-party.txt) holds these two
# lines (innoextract_pins prints them; CMR2005 verified them):
#
#   innoextract <zip url> <sha256> <fallback url> mirror
#   innoextract-exe - <sha256 of innoextract.exe inside it>
#
# innoextract_bundle PKG WORK fetches the zip into WORK, checks the exe
# inside it, and writes
#   PKG/components/innoextract.exe
#   PKG/licenses/innoextract/*.txt   (innoextract's zlib licence, and
#                                     Boost, liblzma and libstdc++'s texts)
# Both the Windows zip and the AppImage carry it: the AppImage runs the
# Windows launcher in its Wine, and innoextract.exe with it (pass
# `--payload components --payload licenses` to appimage-wine.sh).
# THIRD_PARTY.md lists it (scripts/launcher-check rule `innoextract`).
#
# Needs unzip. Returns non-zero with a message on stderr; never exits.

INNOEXTRACT_PINS='# innoextract 1.9 (zlib; Boost, liblzma, libstdc++ runtime exception):
# innoextract.exe unpacks the game'"'"'s installer, and its license/ folder ships.
innoextract https://github.com/dscharrer/innoextract/releases/download/1.9/innoextract-1.9-windows.zip 6989342c9b026a00a72a38f23b62a8e6a22cc5de69805cf47d68ac2fec993065 https://constexpr.org/innoextract/files/innoextract-1.9-windows.zip mirror
innoextract-exe - 946bd06d8b3722a791fea4d84f496139eef6faef61640924cb72f5f1998a68e1'

# Prints the pin lines (with their comment) for packaging/third-party.txt.
innoextract_pins() {
  printf '%s\n' "$INNOEXTRACT_PINS"
}

innoextract_bundle() {
  [ $# -eq 2 ] || { echo "usage: innoextract_bundle PKG_DIR WORK_DIR" >&2; return 2; }
  command -v unzip >/dev/null 2>&1 || { echo "innoextract: unzip is not installed" >&2; return 1; }
  _inno_zip=$(pin_fetch innoextract "$2") || return 1
  _inno_exe_sha=$(pin_sha innoextract-exe) || return 1
  _inno_tmp=$2/innoextract-unzip
  rm -rf "$_inno_tmp"
  mkdir -p "$_inno_tmp" || return 1
  unzip -q -o "$_inno_zip" innoextract.exe 'license/*' -d "$_inno_tmp" \
    || { echo "innoextract: cannot unzip $_inno_zip" >&2; return 1; }
  pin_verify "$_inno_tmp/innoextract.exe" "$_inno_exe_sha" || return 1
  ls "$_inno_tmp"/license/*.txt >/dev/null 2>&1 \
    || { echo "innoextract: the zip has no license/*.txt" >&2; return 1; }
  mkdir -p "$1/components" "$1/licenses/innoextract" || return 1
  cp "$_inno_tmp/innoextract.exe" "$1/components/innoextract.exe" || return 1
  cp "$_inno_tmp"/license/*.txt "$1/licenses/innoextract/" || return 1
  rm -rf "$_inno_tmp"
}
