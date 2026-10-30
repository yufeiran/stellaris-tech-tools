#!/bin/sh
tte_folder=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd) || exit 1
"$tte_folder/tte-core" --build-wizard "$@"
tte_status=$?
if [ -t 0 ] && [ -z "${TTE_CORE_NO_PAUSE:-}" ]; then
    printf '\nPress Enter to exit / 按回车退出: '
    IFS= read -r tte_pause
fi
exit "$tte_status"
