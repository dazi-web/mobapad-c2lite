#!/bin/sh
# Baut die C2-Lite-Firmware aus dem Jieli-SDK (lokal unter sdk-arbeit/, nicht im Repo).
# SDK: github.com/Jieli-Tech/fw-AC63_BT_SDK @ fdc018de + sdk-arbeit/sdk-aenderungen.patch
# Toolchain: pkgman.jieliapp.com/s/linux-toolchain (20260730.1) nach sdk-arbeit/jltc/tc/
# Ergebnis: sdk-arbeit/jieli-sdk/sdk/cpu/br23/tools/sdk.elf (Ladeadresse 0x1E00120, neues Flash-Format).
# Das Abbild für den Flash entsteht NICHT durch Umpacken in Mobapads Rahmen (das hat v1 gebrickt).
set -e
HIER=$(cd "$(dirname "$0")/.." && pwd)
SDK=$HIER/sdk-arbeit/jieli-sdk/sdk
TC=$HIER/sdk-arbeit/jltc/tc/jieli-linux-toolchains-20260730.1
export LD_LIBRARY_PATH=$TC/common/lib:$TC/pi32v2/lib
make -C "$SDK/apps/hid/board/br23" TOOL_DIR="$TC/pi32v2/bin" -j"$(nproc)" "$@"
"$TC/pi32v2/bin/objdump" -h "$SDK/cpu/br23/tools/sdk.elf" | grep " .text"
