#!/bin/sh
set -e

cd "$(dirname "${0}")/.." || exit 1

cargo clippy --workspace --fix --allow-dirty --allow-staged
./hack/format.sh
