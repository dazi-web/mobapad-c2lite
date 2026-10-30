#!/bin/sh
# Packt sdk.elf zu einem Flash-Abbild (jl_isd.bin) mit Jielis isd_download.exe unter Wine (Docker, ohne Gerät).
# app.bin wie cpu/br23/tools/download.bat; Verschlüsselung mit dem Chipkey über sdk-arbeit/schluessel/chipkey_a80f.key.
# Ergebnis: sdk-arbeit/paket/<name>/jl_isd.bin (Pfad steht in sdk-arbeit/paket/letztes). Vorher: werkzeuge/sdk-bauen.sh
#   ~/mobapad-c2lite/werkzeuge/abbild_packen.sh [name]
set -e
S=$(cd "$(dirname "$0")/../sdk-arbeit" && pwd)
T=$S/jieli-sdk/sdk/cpu/br23/tools
TC=$S/jltc/tc/jieli-linux-toolchains-20260730.1
P=paket/${1:-abbild}-$(date +%m%d-%H%M)
mkdir -p "$S/$P"
export LD_LIBRARY_PATH=$TC/common/lib:$TC/pi32v2/lib
for s in text data data_code overlay_aec overlay_wav overlay_ape overlay_flac overlay_m4a overlay_amr overlay_dts \
         overlay_fm overlay_mp3 overlay_wma; do
    "$TC/pi32v2/bin/objcopy" -O binary -j ".$s" "$T/sdk.elf" "$S/$P/$s.bin" 2>/dev/null || : > "$S/$P/$s.bin"
done
python3 -I - "$S/$P" <<'PY'
import os, sys
p = sys.argv[1]
r = lambda n: open(os.path.join(p, n + ".bin"), "rb").read()
trim = lambda b: b.rstrip(b"\xff")  # remove_tailing_zeros -mark ff
app = b"".join([r("text"), r("data"), r("data_code"), trim(r("overlay_aec")), r("overlay_wav"), r("overlay_ape"),
                r("overlay_flac"), r("overlay_m4a"), r("overlay_amr"), r("overlay_dts"), trim(r("overlay_fm")),
                trim(r("overlay_mp3")), trim(r("overlay_wma"))])
open(os.path.join(p, "app.bin"), "wb").write(app)
print("app.bin", len(app))
PY
cp "$T/isd_config.ini" "$T/uboot.boot" "$T/br23loader.bin" "$T/cfg_tool.bin" "$T/flash_params.bin" "$T/ota_all.bin" \
   "$T/isd_download.exe" "$S/schluessel/chipkey_a80f.key" "$S/$P/"
chmod -R a+rwX "$S/$P"
cd "$S/$P"
timeout 300 docker run --rm -v "$PWD":/work -w /work scottyhardy/docker-wine:stable wine isd_download.exe isd_config.ini \
    -tonorflash -dev br23 -boot 0x12000 -div8 -wait 300 -uboot uboot.boot -app app.bin cfg_tool.bin \
    -ota_file ota_all.bin -flash-params flash_params.bin -key chipkey_a80f.key > isd_download.log 2>&1 || true
[ -s jl_isd.bin ] || { echo "FEHLER: kein jl_isd.bin, siehe $S/$P/isd_download.log"; exit 1; }
echo "$P" > "$S/paket/letztes"
echo "OK: $S/$P/jl_isd.bin"
