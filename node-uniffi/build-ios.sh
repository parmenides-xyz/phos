#!/usr/bin/env bash

set -euxo pipefail

cd -- "$(dirname -- "${BASH_SOURCE[0]}")"

export IPHONEOS_DEPLOYMENT_TARGET="${IPHONEOS_DEPLOYMENT_TARGET:-18.0}"

# create swift bindings

rm -rf ./bindings ./ios
mkdir -p ./bindings
mkdir -p ./ios
mkdir -p ./bindings/Headers

cargo build

cargo run --bin uniffi-bindgen \
  generate \
  --library ../target/debug/libphos_node_uniffi.dylib \
  --language swift \
  --out-dir ./bindings

cat \
	./bindings/phos_node_uniffiFFI.modulemap > ./bindings/Headers/module.modulemap

cp ./bindings/*.h ./bindings/Headers/

rm -rf ./ios/phos.xcframework

# create xcode project

cargo build -p phos-node-uniffi \
  --release \
  --lib \
  --target aarch64-apple-ios \
  --target aarch64-apple-ios-sim

xcodebuild -create-xcframework \
  -library ../target/aarch64-apple-ios/release/libphos_node_uniffi.a -headers ./bindings/Headers \
  -library ../target/aarch64-apple-ios-sim/release/libphos_node_uniffi.a -headers ./bindings/Headers \
  -output "ios/phos.xcframework"

cp ./bindings/*.swift ./ios/

rm -rf bindings
