#!/usr/bin/env python3
"""Write ``top100_vcpkg_ports.txt`` from a curated candidate list (HTTP-verified)."""

from __future__ import annotations

import urllib.error
import urllib.request
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[3]
OUT = REPO_ROOT / "benchmarks" / "calibration" / "top100_vcpkg_ports.txt"
USER_AGENT = "topos-calibration (https://github.com/krv-ai/topos; contact@krv.ai)"

# Superset to draw 100 verified ports from (order preserved).
CANDIDATES = (  # noqa: SIM905
    "zlib openssl curl fmt spdlog sqlite3 protobuf grpc abseil "
    "eigen3 opencv4 catch2 gtest "
    "nlohmann-json yaml-cpp boost-filesystem boost-system boost-thread "
    "boost-asio boost-beast "
    "boost-log boost-iostreams boost-program-options boost-test "
    "boost-date-time boost-chrono "
    "boost-serialization boost-regex boost-random boost-uuid boost-url "
    "boost-json "
    "boost-variant2 boost-container boost-geometry boost-graph boost-wave "
    "boost-locale "
    "boost-fiber boost-context boost-coroutine2 boost-process "
    "boost-interprocess expat libxml2 "
    "libxslt libpng libjpeg-turbo freetype harfbuzz pcre2 re2 icu brotli "
    "bzip2 lz4 zstd snappy "
    "leveldb rocksdb hiredis libmysql sqlitecpp sqlpp11 oatpp cpprestsdk "
    "entt glm rapidjson "
    "wil ms-gsl date zeromq cppzmq assimp bullet3 glfw3 glew sdl2 imgui "
    "stb magic-enum "
    "range-v3 libssh2 nghttp2 nghttp3 ngtcp2 c-ares thrift libpqxx libpq "
    "mongo-cxx-driver "
    "redis-plus-plus drogon poco libuv protobuf-c flatbuffers capnproto "
    "arrow aws-sdk-cpp "
    "azure-core-cpp google-cloud-cpp onnxruntime tensorflow-lite opencv2 "
    "vcpkg-cmake "
    "vcpkg-tool-meson sqlite-orm mongoose cpp-httplib"
).split()  # noqa: SIM905


def exists(port: str) -> bool:
    url = f"https://raw.githubusercontent.com/microsoft/vcpkg/master/ports/{port}/vcpkg.json"
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(req, timeout=20) as resp:
            return resp.status == 200
    except urllib.error.HTTPError:
        return False
    except OSError:
        return False


def main() -> None:
    ok: list[str] = []
    for p in CANDIDATES:
        if p in ok:
            continue
        if exists(p):
            ok.append(p)
        if len(ok) >= 100:
            break
    header = (
        "# Top-100 vcpkg ports (auto-generated; HTTP-verified on master).\n"
        "# Regenerate: refresh_top100_vcpkg_ports.py\n"
    )
    OUT.write_text(header + "\n".join(ok) + "\n", encoding="utf-8")
    print(f"Wrote {len(ok)} ports to {OUT}")


if __name__ == "__main__":
    main()
