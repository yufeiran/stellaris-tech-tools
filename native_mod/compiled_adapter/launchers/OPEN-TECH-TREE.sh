#!/bin/sh
tte_folder=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd) || exit 1
tte_os=$(uname -s) || exit 1
tte_cpu=$(uname -m) || exit 1
case "$tte_os/$tte_cpu" in
    Darwin/arm64|Darwin/aarch64) tte_platform=macOS-arm64 ;;
    Darwin/x86_64) tte_platform=macOS-x86_64 ;;
    Linux/x86_64|Linux/amd64) tte_platform=Linux-x86_64-musl ;;
    *) printf 'Unsupported system / CPU: %s / %s\n' "$tte_os" "$tte_cpu" >&2; exit 1 ;;
esac
tte_core="$tte_folder/technology-adapter/$tte_platform/tte-core"
if [ ! -x "$tte_core" ]; then
    chmod u+x "$tte_core" || exit 1
fi
"$tte_core" --web-wizard "$@"
tte_status=$?
if [ -t 0 ] && [ -z "${TTE_CORE_NO_PAUSE:-}" ]; then
    printf '\nPress Enter to exit / 按回车退出: '
    IFS= read -r tte_pause
fi
exit "$tte_status"
