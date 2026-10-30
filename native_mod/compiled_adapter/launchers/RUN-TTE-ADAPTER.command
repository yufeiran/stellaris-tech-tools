#!/bin/sh
tte_folder=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd) || exit 1
exec /bin/sh "$tte_folder/RUN-TTE-ADAPTER.sh" "$@"
