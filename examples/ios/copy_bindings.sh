#!/usr/bin/env bash

set -euxo pipefail

cd "$(git rev-parse --show-toplevel)"

cp -rv node-uniffi/ios/phos.xcframework examples/ios
cp -v node-uniffi/ios/*.swift examples/ios/DataNetworkDemo
