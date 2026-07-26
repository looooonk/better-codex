#!/usr/bin/env bash
set -euo pipefail

: "${TARGET:?TARGET environment variable is required}"
: "${GITHUB_ENV:?GITHUB_ENV environment variable is required}"

sudo apt-get update
sudo apt-get install -y ca-certificates curl musl-tools pkg-config libcap-dev g++ clang libc++-dev libc++abi-dev lld xz-utils

case "${TARGET}" in
  x86_64-unknown-linux-musl)
    arch="x86_64"
    ;;
  aarch64-unknown-linux-musl)
    arch="aarch64"
    ;;
  *)
    echo "Unexpected musl target: ${TARGET}" >&2
    exit 1
    ;;
esac

libcap_version="2.75"
libcap_sha256="de4e7e064c9ba451d5234dd46e897d7c71c96a9ebf9a0c445bc04f4742d83632"
libcap_tarball_name="libcap-${libcap_version}.tar.xz"
libcap_download_url="https://mirrors.edge.kernel.org/pub/linux/libs/security/linux-privs/libcap2/${libcap_tarball_name}"

if command -v "${arch}-linux-musl-gcc" >/dev/null; then
  musl_linker="$(command -v "${arch}-linux-musl-gcc")"
elif command -v musl-gcc >/dev/null; then
  musl_linker="$(command -v musl-gcc)"
else
  echo "musl gcc not found after install; arch=${arch}" >&2
  exit 1
fi

zig_target="${TARGET/-unknown-linux-musl/-linux-musl}"
runner_temp="${RUNNER_TEMP:-/tmp}"
tool_root="${runner_temp}/better-codex-musl-tools-${TARGET}"
mkdir -p "${tool_root}"

libcap_root="${tool_root}/libcap-${libcap_version}"
libcap_src_root="${libcap_root}/src"
libcap_prefix="${libcap_root}/prefix"
libcap_pkgconfig_dir="${libcap_prefix}/lib/pkgconfig"

if [[ ! -f "${libcap_prefix}/lib/libcap.a" ]]; then
  mkdir -p "${libcap_src_root}" "${libcap_prefix}/lib" "${libcap_prefix}/include/sys" "${libcap_prefix}/include/linux" "${libcap_pkgconfig_dir}"
  libcap_tarball="${libcap_root}/${libcap_tarball_name}"

  curl -fsSL "${libcap_download_url}" -o "${libcap_tarball}"
  echo "${libcap_sha256}  ${libcap_tarball}" | sha256sum -c -

  tar -xJf "${libcap_tarball}" -C "${libcap_src_root}"
  libcap_source_dir="${libcap_src_root}/libcap-${libcap_version}"
  make -C "${libcap_source_dir}/libcap" -j"$(nproc)" \
    CC="${musl_linker}" \
    AR=ar \
    RANLIB=ranlib

  cp "${libcap_source_dir}/libcap/libcap.a" "${libcap_prefix}/lib/libcap.a"
  cp "${libcap_source_dir}/libcap/include/uapi/linux/capability.h" "${libcap_prefix}/include/linux/capability.h"
  cp "${libcap_source_dir}/libcap/../libcap/include/sys/capability.h" "${libcap_prefix}/include/sys/capability.h"

  cat >"${libcap_pkgconfig_dir}/libcap.pc" <<EOF
prefix=${libcap_prefix}
exec_prefix=\${prefix}
libdir=\${prefix}/lib
includedir=\${prefix}/include

Name: libcap
Description: Linux capabilities
Version: ${libcap_version}
Libs: -L\${libdir} -lcap
Cflags: -I\${includedir}
EOF
fi

sysroot=""
if command -v zig >/dev/null; then
  zig_bin="$(command -v zig)"
  cc="${tool_root}/zigcc"
  cxx="${tool_root}/zigcxx"

  cat >"${cc}" <<EOF
#!/usr/bin/env bash
set -euo pipefail

args=()
skip_next=0
pending_include=0
for arg in "\$@"; do
  if [[ "\${pending_include}" -eq 1 ]]; then
    pending_include=0
    if [[ "\${arg}" == /usr/include || "\${arg}" == /usr/include/* ]]; then
      args+=("-idirafter" "\${arg}")
    else
      args+=("-I" "\${arg}")
    fi
    continue
  fi

  if [[ "\${skip_next}" -eq 1 ]]; then
    skip_next=0
    continue
  fi
  case "\${arg}" in
    --target)
      skip_next=1
      continue
      ;;
    --target=*|-target=*|-target)
      if [[ "\${arg}" == "-target" ]]; then
        skip_next=1
      fi
      continue
      ;;
    -I)
      pending_include=1
      continue
      ;;
    -I/usr/include|-I/usr/include/*)
      args+=("-idirafter" "\${arg#-I}")
      continue
      ;;
    -Wp,-U_FORTIFY_SOURCE)
      args+=("-U_FORTIFY_SOURCE")
      continue
      ;;
  esac
  args+=("\${arg}")
done

exec "${zig_bin}" cc -target "${zig_target}" "\${args[@]}" -fno-sanitize=undefined
EOF
  cat >"${cxx}" <<EOF
#!/usr/bin/env bash
set -euo pipefail

args=()
skip_next=0
pending_include=0
for arg in "\$@"; do
  if [[ "\${pending_include}" -eq 1 ]]; then
    pending_include=0
    if [[ "\${arg}" == /usr/include || "\${arg}" == /usr/include/* ]]; then
      args+=("-idirafter" "\${arg}")
    else
      args+=("-I" "\${arg}")
    fi
    continue
  fi

  if [[ "\${skip_next}" -eq 1 ]]; then
    skip_next=0
    continue
  fi
  case "\${arg}" in
    --target)
      skip_next=1
      continue
      ;;
    --target=*|-target=*|-target)
      if [[ "\${arg}" == "-target" ]]; then
        skip_next=1
      fi
      continue
      ;;
    -I)
      pending_include=1
      continue
      ;;
    -I/usr/include|-I/usr/include/*)
      args+=("-idirafter" "\${arg#-I}")
      continue
      ;;
    -Wp,-U_FORTIFY_SOURCE)
      args+=("-U_FORTIFY_SOURCE")
      continue
      ;;
  esac
  args+=("\${arg}")
done

exec "${zig_bin}" c++ -target "${zig_target}" "\${args[@]}" -fno-sanitize=undefined
EOF
  chmod +x "${cc}" "${cxx}"

  sysroot="$("${zig_bin}" cc -target "${zig_target}" -print-sysroot 2>/dev/null || true)"
else
  cc="${musl_linker}"

  if command -v "${arch}-linux-musl-g++" >/dev/null; then
    cxx="$(command -v "${arch}-linux-musl-g++")"
  elif command -v musl-g++ >/dev/null; then
    cxx="$(command -v musl-g++)"
  else
    cxx="${cc}"
  fi
fi

if [[ -n "${sysroot}" && "${sysroot}" != "/" ]]; then
  echo "BORING_BSSL_SYSROOT=${sysroot}" >>"$GITHUB_ENV"
  boring_sysroot_var="BORING_BSSL_SYSROOT_${TARGET}"
  boring_sysroot_var="${boring_sysroot_var//-/_}"
  echo "${boring_sysroot_var}=${sysroot}" >>"$GITHUB_ENV"
fi

cflags="-pthread"
cxxflags="-pthread"
if [[ "${TARGET}" == "aarch64-unknown-linux-musl" ]]; then
  cflags="${cflags} -Wno-error=frame-larger-than"
  cxxflags="${cxxflags} -Wno-error=frame-larger-than"
fi

target_cc_var="CC_${TARGET}"
target_cc_var="${target_cc_var//-/_}"
target_cxx_var="CXX_${TARGET}"
target_cxx_var="${target_cxx_var//-/_}"
cargo_linker_var="CARGO_TARGET_${TARGET^^}_LINKER"
cargo_linker_var="${cargo_linker_var//-/_}"
{
  echo "CFLAGS=${cflags}"
  echo "CXXFLAGS=${cxxflags}"
  echo "CC=${cc}"
  echo "TARGET_CC=${cc}"
  echo "${target_cc_var}=${cc}"
  echo "CXX=${cxx}"
  echo "TARGET_CXX=${cxx}"
  echo "${target_cxx_var}=${cxx}"
  echo "${cargo_linker_var}=${musl_linker}"
  echo "CMAKE_C_COMPILER=${cc}"
  echo "CMAKE_CXX_COMPILER=${cxx}"
  echo "CMAKE_ARGS=-DCMAKE_HAVE_THREADS_LIBRARY=1 -DCMAKE_USE_PTHREADS_INIT=1 -DCMAKE_THREAD_LIBS_INIT=-pthread -DTHREADS_PREFER_PTHREAD_FLAG=ON"
} >>"$GITHUB_ENV"

echo "PKG_CONFIG_ALLOW_CROSS=1" >>"$GITHUB_ENV"
pkg_config_path="${libcap_pkgconfig_dir}"
if [[ -n "${PKG_CONFIG_PATH:-}" ]]; then
  pkg_config_path="${pkg_config_path}:${PKG_CONFIG_PATH}"
fi
pkg_config_path_var="PKG_CONFIG_PATH_${TARGET}"
pkg_config_path_var="${pkg_config_path_var//-/_}"
pkg_config_libdir_var="PKG_CONFIG_LIBDIR_${TARGET}"
pkg_config_libdir_var="${pkg_config_libdir_var//-/_}"
{
  echo "PKG_CONFIG_PATH=${pkg_config_path}"
  echo "${pkg_config_path_var}=${libcap_pkgconfig_dir}"
  echo "${pkg_config_libdir_var}=${libcap_pkgconfig_dir}"
} >>"$GITHUB_ENV"

if [[ -n "${sysroot}" && "${sysroot}" != "/" ]]; then
  echo "PKG_CONFIG_SYSROOT_DIR=${sysroot}" >>"$GITHUB_ENV"
  pkg_config_sysroot_var="PKG_CONFIG_SYSROOT_DIR_${TARGET}"
  pkg_config_sysroot_var="${pkg_config_sysroot_var//-/_}"
  echo "${pkg_config_sysroot_var}=${sysroot}" >>"$GITHUB_ENV"
fi
