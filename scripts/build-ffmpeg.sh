#!/bin/bash
# Build an LGPL-only ffmpeg/ffprobe (with libmp3lame and libsoxr) as a
# universal macOS bundle for Bake'n Deck for Mac.
#
# Output layout (build/ffmpeg-dist):
#   Helpers/ffmpeg, Helpers/ffprobe       universal executables, rpath = @executable_path/../Frameworks
#   Frameworks/lib*.dylib                 universal shared libraries, install name = @rpath/<name>
#   BUILDINFO.txt                         versions, hashes and configure flags for the LGPL source offer
#
# Requirements: Xcode command line tools, nasm, cmake, pkg-config (brew install nasm cmake pkg-config).
#
# This script is published verbatim at
# https://github.com/M-Igashi/baken/blob/main/scripts/build-ffmpeg.sh so the LGPL
# source offer on https://baken.ravers.workers.dev/third-party can point at it.
# Keep the two copies identical.
set -euo pipefail

FFMPEG_VERSION=9.0.1
FFMPEG_SHA256=cf38e0e28c7e5605942c4a77755349b0145804a397af37eb1fb4c77cb237f635
LAME_VERSION=3.100
LAME_SHA256=ddfe36cab873794038ae2c1210557ad34857a4b6bdc515785d1da9e175b1da1e
SOXR_VERSION=0.1.3
SOXR_SHA256=b111c15fdc8c029989330ff559184198c161100a59312f5dc19ddeb9b5a15889
MACOS_MIN=14.0

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUILD="$ROOT/build"
SRC="$BUILD/src"
DIST="$BUILD/ffmpeg-dist"
ARCHES="${ARCHES:-arm64 x86_64}"
JOBS="$(sysctl -n hw.ncpu)"

mkdir -p "$SRC"

fetch() { # url sha256 out
  local url="$1" sha="$2" out="$SRC/$3"
  if [ ! -f "$out" ]; then
    echo "fetch $3"
    curl -fsSL "$url" -o "$out"
  fi
  echo "$sha  $out" | shasum -a 256 -c - >/dev/null || { echo "checksum mismatch for $3"; exit 1; }
}

fetch "https://ffmpeg.org/releases/ffmpeg-$FFMPEG_VERSION.tar.xz" "$FFMPEG_SHA256" "ffmpeg-$FFMPEG_VERSION.tar.xz"
fetch "https://sourceforge.net/projects/lame/files/lame/$LAME_VERSION/lame-$LAME_VERSION.tar.gz/download" "$LAME_SHA256" "lame-$LAME_VERSION.tar.gz"
fetch "https://sourceforge.net/projects/soxr/files/soxr-$SOXR_VERSION-Source.tar.xz/download" "$SOXR_SHA256" "soxr-$SOXR_VERSION-Source.tar.xz"

# Everything baken actually uses (see baken-core: analyzer, processor, cdjsafe/transcode).
FFMPEG_FLAGS=(
  --disable-everything --disable-programs --enable-ffmpeg --enable-ffprobe
  --disable-doc --disable-debug --disable-autodetect --disable-network
  --disable-static --enable-shared --enable-pic --disable-avdevice
  --disable-gpl --disable-nonfree --disable-version3
  --enable-libmp3lame --enable-libsoxr
  --enable-protocol=file,pipe
  --enable-demuxer=flac,wav,aiff,mp3,mov,aac,image2,image_jpeg_pipe,image_png_pipe
  --enable-muxer=flac,wav,aiff,mp3,ipod,mp4,null,image2
  --enable-decoder=flac,mp3,mp3float,aac,alac,mjpeg,png
  --enable-decoder=pcm_s8,pcm_u8,pcm_s16le,pcm_s16be,pcm_s24le,pcm_s24be,pcm_s32le,pcm_s32be,pcm_f32le,pcm_f32be,pcm_f64le,pcm_f64be
  --enable-encoder=flac,aac,alac,libmp3lame,mjpeg
  --enable-encoder=pcm_s8,pcm_u8,pcm_s16le,pcm_s16be,pcm_s24le,pcm_s24be,pcm_s32le,pcm_s32be,pcm_f32le,pcm_f32be,pcm_f64le,pcm_f64be
  --enable-parser=flac,mpegaudio,aac,mjpeg,png
  --enable-bsf=aac_adtstoasc
  --enable-filter=loudnorm,volume,aresample,aformat,anull,format,null,scale,trim,atrim,copy
  --enable-swresample --enable-swscale
  # png needs zlib; --disable-autodetect hides the system copy unless asked for.
  --enable-zlib
)

build_arch() {
  local arch="$1"
  local prefix="$BUILD/prefix-$arch"
  local host
  case "$arch" in
    arm64) host=aarch64-apple-darwin ;;
    x86_64) host=x86_64-apple-darwin ;;
  esac
  local cflags="-arch $arch -mmacosx-version-min=$MACOS_MIN -O2"
  rm -rf "$prefix" "$BUILD/work-$arch"
  mkdir -p "$prefix" "$BUILD/work-$arch"

  echo "== [$arch] lame"
  tar -xzf "$SRC/lame-$LAME_VERSION.tar.gz" -C "$BUILD/work-$arch"
  ( cd "$BUILD/work-$arch/lame-$LAME_VERSION"
    # lame 3.100 exports a symbol it no longer defines; the linker rejects the .sym file otherwise.
    sed -i '' '/lame_init_old/d' include/libmp3lame.sym
    CC=clang CFLAGS="$cflags" LDFLAGS="-arch $arch" ./configure --host="$host" --prefix="$prefix" \
      --disable-static --enable-shared --disable-frontend --disable-gtktest >/dev/null
    make -j"$JOBS" >/dev/null && make install >/dev/null )

  echo "== [$arch] soxr"
  tar -xJf "$SRC/soxr-$SOXR_VERSION-Source.tar.xz" -C "$BUILD/work-$arch"
  ( cd "$BUILD/work-$arch/soxr-$SOXR_VERSION-Source" && mkdir -p b && cd b
    cmake .. -Wno-dev -DCMAKE_POLICY_VERSION_MINIMUM=3.5 -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$prefix" -DCMAKE_OSX_ARCHITECTURES="$arch" \
      -DCMAKE_OSX_DEPLOYMENT_TARGET="$MACOS_MIN" -DBUILD_SHARED_LIBS=ON -DBUILD_TESTS=OFF -DBUILD_EXAMPLES=OFF \
      -DWITH_OPENMP=OFF -DWITH_LSR_BINDINGS=OFF >/dev/null
    make -j"$JOBS" >/dev/null && make install >/dev/null )

  echo "== [$arch] ffmpeg"
  tar -xJf "$SRC/ffmpeg-$FFMPEG_VERSION.tar.xz" -C "$BUILD/work-$arch"
  ( cd "$BUILD/work-$arch/ffmpeg-$FFMPEG_VERSION"
    local cross=()
    if [ "$arch" != "$(uname -m)" ]; then
      cross=(--enable-cross-compile --arch="$arch" --target-os=darwin)
    fi
    PKG_CONFIG_PATH="$prefix/lib/pkgconfig" ./configure --prefix="$prefix" \
      --cc=clang --extra-cflags="$cflags -I$prefix/include" --extra-ldflags="-arch $arch -L$prefix/lib" \
      --install-name-dir=@rpath ${cross[@]+"${cross[@]}"} "${FFMPEG_FLAGS[@]}" >/dev/null
    make -j"$JOBS" >/dev/null && make install >/dev/null )
}

if [ -z "${ASSEMBLE_ONLY:-}" ]; then
  for arch in $ARCHES; do
    build_arch "$arch"
  done
fi

echo "== assemble universal dist"
rm -rf "$DIST"
mkdir -p "$DIST/Helpers" "$DIST/Frameworks"
first_arch="${ARCHES%% *}"
for lib in "$BUILD/prefix-$first_arch"/lib/*.dylib; do
  name="$(basename "$lib")"
  [ -L "$lib" ] && continue   # only real files; symlinks are re-created below
  inputs=(); for arch in $ARCHES; do inputs+=("$BUILD/prefix-$arch/lib/$name"); done
  lipo -create "${inputs[@]}" -output "$DIST/Frameworks/$name"
done
for lib in "$BUILD/prefix-$first_arch"/lib/*.dylib; do
  name="$(basename "$lib")"
  if [ -L "$lib" ]; then ln -sf "$(readlink "$lib")" "$DIST/Frameworks/$name"; fi
done
for bin in ffmpeg ffprobe; do
  inputs=(); for arch in $ARCHES; do inputs+=("$BUILD/prefix-$arch/bin/$bin"); done
  lipo -create "${inputs[@]}" -output "$DIST/Helpers/$bin"
done

# Make every install name @rpath-relative (lame and soxr are installed with absolute paths)
fix_names() {
  local f="$1"
  # Fat binaries list one section per architecture, each with its own prefix path.
  for dep in $(otool -L "$f" | awk 'NR>1 {print $1}' | grep "$BUILD/prefix" | sort -u || true); do
    install_name_tool -change "$dep" "@rpath/$(basename "$dep")" "$f"
  done
}
for f in "$DIST"/Frameworks/*.dylib; do
  [ -L "$f" ] && continue
  install_name_tool -id "@rpath/$(basename "$f")" "$f"
  fix_names "$f"
done
for bin in "$DIST"/Helpers/*; do
  fix_names "$bin"
  install_name_tool -add_rpath "@executable_path/../Frameworks" "$bin"
done

echo "== verify"
lipo -info "$DIST/Helpers/ffmpeg"
"$DIST/Helpers/ffmpeg" -hide_banner -L | head -3
if "$DIST/Helpers/ffmpeg" -hide_banner -buildconf | grep -q -- '--enable-gpl'; then
  echo "ERROR: GPL component enabled"; exit 1
fi
"$DIST/Helpers/ffmpeg" -hide_banner -L | grep -q 'Lesser General Public' || { echo "ERROR: license banner is not LGPL"; exit 1; }
for f in "$DIST"/Helpers/* "$DIST"/Frameworks/*.dylib; do
  if otool -L "$f" | awk 'NR>1 {print $1}' | grep -q "$BUILD/prefix"; then echo "ERROR: absolute build path left in $f"; exit 1; fi
done

{
  echo "ffmpeg $FFMPEG_VERSION  sha256 $FFMPEG_SHA256  https://ffmpeg.org/releases/ffmpeg-$FFMPEG_VERSION.tar.xz  (LGPL 2.1 or later)"
  echo "lame $LAME_VERSION  sha256 $LAME_SHA256  https://sourceforge.net/projects/lame/files/lame/$LAME_VERSION/  (LGPL 2.0 or later)"
  echo "soxr $SOXR_VERSION  sha256 $SOXR_SHA256  https://sourceforge.net/projects/soxr/files/  (LGPL 2.1 or later)"
  echo "configure: ${FFMPEG_FLAGS[*]}"
  echo "arches: $ARCHES  macos-min: $MACOS_MIN"
} > "$DIST/BUILDINFO.txt"
echo "done: $DIST"
