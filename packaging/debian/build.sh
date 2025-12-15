#!/bin/bash
set -e

# Build the Rust application
echo "Building kdownload..."
cargo build --release

# Create the package directory structure
echo "Creating package structure..."
mkdir -p packaging/debian/package/usr/local/bin
mkdir -p packaging/debian/package/usr/share/doc/kdownload

# Copy the binary and documentation
echo "Copying files..."
cp target/release/kdownload packaging/debian/package/usr/local/bin/
cp README.md packaging/debian/package/usr/share/doc/kdownload/README.md
cp LICENSE packaging/debian/package/usr/share/doc/kdownload/LICENSE

# Set permissions
echo "Setting permissions..."
chmod 755 packaging/debian/package/usr/local/bin/kdownload
chown -R root:root packaging/debian/package/

# Build the .deb package
echo "Building .deb package..."
fakeroot dpkg-deb --build packaging/debian/package kdownload_0.1.2_amd64.deb

echo "Build complete."
