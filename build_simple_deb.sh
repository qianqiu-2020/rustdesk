#!/bin/bash
# RustDesk Simple DEB Package Builder on k1
# Based on build_deb_from_folder function logic
# Package target/release/rustdesk binary into DEB package

set -e

# Get script directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Configuration variables
BUILD_MODE="${BUILD_MODE:-debug}"  # debug or release
VERSION="1.4.2"
DEB_ARCH="${DEB_ARCH:-riscv64}"
WORK_DIR="tmpdeb"

# Set binary file path based on build mode
if [ "$BUILD_MODE" = "release" ]; then
    BINARY_PATH="target/release/rustdesk"
else
    BINARY_PATH="target/debug/rustdesk"
fi

# Color output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Logging functions
log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Cleanup function
cleanup() {
    if [ -d "$WORK_DIR" ]; then
        log_info "Cleaning temporary directory $WORK_DIR"
        rm -rf "$WORK_DIR"
    fi
}

# Error handling
error_exit() {
    log_error "$1"
    cleanup
    exit 1
}

# Trap error signals
trap cleanup EXIT

# Check dependencies
check_dependencies() {
    log_info "Checking build dependencies..."
    
    if ! command -v dpkg-deb &> /dev/null; then
        error_exit "dpkg-deb not found, please install dpkg tools"
    fi
    
    if [ ! -f "$BINARY_PATH" ]; then
        error_exit "Binary file $BINARY_PATH does not exist, please build the project first"
    fi
    
    if [ ! -x "$BINARY_PATH" ]; then
        error_exit "$BINARY_PATH is not executable"
    fi
    
    log_info "Dependency check completed"
}

# Get extra dependencies
get_deb_extra_depends() {
    if [ "$DEB_ARCH" = "armhf" ]; then
        echo ", libatomic1"
    else
        echo ""
    fi
}

# Get compression settings - use gzip fast compression
get_compression_settings() {
    local cpu_count
    cpu_count=$(nproc 2>/dev/null || echo "1")
    
    # Use zstd compression, level 3 provides good balance of speed and compression ratio
    echo "zstd" "$cpu_count" "3"
}

# Create directory structure
create_directory_structure() {
    log_info "Creating DEB package directory structure..."
    
    # Clean and create working directory
    rm -rf "$WORK_DIR"
    
    # Create standard DEB directory structure
    mkdir -p "$WORK_DIR/usr/bin/"
    mkdir -p "$WORK_DIR/usr/share/rustdesk"
    mkdir -p "$WORK_DIR/usr/share/rustdesk/files/systemd/"
    mkdir -p "$WORK_DIR/usr/share/icons/hicolor/256x256/apps/"
    mkdir -p "$WORK_DIR/usr/share/icons/hicolor/scalable/apps/"
    mkdir -p "$WORK_DIR/usr/share/applications/"
    mkdir -p "$WORK_DIR/usr/share/polkit-1/actions"
    mkdir -p "$WORK_DIR/etc/rustdesk/"
    mkdir -p "$WORK_DIR/etc/pam.d/"
    mkdir -p "$WORK_DIR/DEBIAN"
    
    log_info "Directory structure creation completed"
}

# Copy binary files and resources
copy_files() {
    log_info "Copying binary files and resources..."
    
    # Copy main binary file to /usr/share/rustdesk/
    cp "$BINARY_PATH" "$WORK_DIR/usr/share/rustdesk/"
    chmod 755 "$WORK_DIR/usr/share/rustdesk/rustdesk"
    
    # Create symbolic link under /usr/bin/ (will be created in postinst)
    
    # Copy systemd service file
    if [ -f "res/rustdesk.service" ]; then
        cp "res/rustdesk.service" "$WORK_DIR/usr/share/rustdesk/files/systemd/"
    else
        log_warn "Service file res/rustdesk.service does not exist"
    fi
    
    # Copy icon files
    if [ -f "res/128x128@2x.png" ]; then
        cp "res/128x128@2x.png" "$WORK_DIR/usr/share/icons/hicolor/256x256/apps/rustdesk.png"
    else
        log_warn "Icon file res/128x128@2x.png does not exist"
    fi
    
    if [ -f "res/scalable.svg" ]; then
        cp "res/scalable.svg" "$WORK_DIR/usr/share/icons/hicolor/scalable/apps/rustdesk.svg"
    else
        log_warn "Vector icon file res/128x128@2x.png does not exist"
    fi
    
    # # Copy desktop launch files
    # if [ -f "res/rustdesk.desktop" ]; then
    #     cp "res/rustdesk.desktop" "$WORK_DIR/usr/share/applications/"
    # else
    #     log_warn "Desktop file res/rustdesk.desktop does not exist"
    # fi
    
    # if [ -f "res/rustdesk-link.desktop" ]; then
    #     cp "res/rustdesk-link.desktop" "$WORK_DIR/usr/share/applications/"
    # else
    #     log_warn "Link desktop file res/rustdesk.desktop does not exist"
    # fi
    
    # Copy configuration files
    if [ -f "res/startwm.sh" ]; then
        cp "res/startwm.sh" "$WORK_DIR/etc/rustdesk/"
    else
        log_warn "Start script res/startwm.sh does not exist"
    fi
    
    if [ -f "res/xorg.conf" ]; then
        cp "res/xorg.conf" "$WORK_DIR/etc/rustdesk/"
    else
        log_warn "Xorg config file res/xorg.conf does not exist"
    fi
    
    # Copy PAM configuration file
    if [ -f "res/pam.d/rustdesk.debian" ]; then
        cp "res/pam.d/rustdesk.debian" "$WORK_DIR/etc/pam.d/rustdesk"
    else
        log_warn "PAM config file res/pam.d/rustdesk.debian does not exist"
    fi
    
    # Create polkit file
    echo "#!/bin/sh" > "$WORK_DIR/usr/share/rustdesk/files/polkit"
    chmod a+x "$WORK_DIR/usr/share/rustdesk/files/polkit"
    
    log_info "File copying completed"
}

# Generate control file
generate_control_file() {
    log_info "Generating DEBIAN/control file..."
    
    local extra_depends
    extra_depends=$(get_deb_extra_depends)
    
    cat > "$WORK_DIR/DEBIAN/control" << EOF
Package: rustdesk
Section: net
Priority: optional
Version: $VERSION
Architecture: $DEB_ARCH
Maintainer: rustdesk <info@rustdesk.com>
Homepage: https://rustdesk.com
Depends: libyuv0, libgtk-3-0, libxcb-randr0, libxdo3, libxfixes3, libxcb-shape0, libxcb-xfixes0, libasound2, libsystemd0, curl, libva2, libva-drm2, libva-x11-2, libgstreamer-plugins-base1.0-0, libpam0g, gstreamer1.0-pipewire$extra_depends
Recommends: libayatana-appindicator3-1
Description: A remote control software.

EOF
    
    log_info "Control file generation completed"
}

# Copy DEBIAN script files
copy_debian_scripts() {
    log_info "Copy DEBIAN script files..."
    
    # Copy all DEBIAN script files
    for script in postinst preinst prerm postrm; do
        if [ -f "res/DEBIAN/$script" ]; then
            cp "res/DEBIAN/$script" "$WORK_DIR/DEBIAN/"
            chmod 755 "$WORK_DIR/DEBIAN/$script"
            log_info "Copied $script script"
        else
            log_warn "DEBIAN script res/DEBIAN/$script does not exist"
        fi
    done
}

# Generate MD5 checksums
generate_md5sums() {
    log_info "Generate MD5 checksums..."
    
    cd "$WORK_DIR"
    find . -type f ! -path "./DEBIAN/*" -exec md5sum {} \; | sed 's|\./|/|' > DEBIAN/md5sums
    cd - > /dev/null
    
    log_info "MD5 checksum generation completed"
}

# Build DEB package
build_deb_package() {
    log_info "Build DEB package..."
    
    local deb_filename="rustdesk-${VERSION}-${DEB_ARCH}.deb"
    
    # Get optimal compression settings
    local compression_settings
    IFS=' ' read -ra compression_settings <<< "$(get_compression_settings)"
    local compressor="${compression_settings[0]}"
    local threads="${compression_settings[1]}"
    local level="${compression_settings[2]}"
    
    log_info "Using zstd compression (level: $level, threads: $threads): $threads)"
    
    # Use dpkg-deb to build package with parallel compression enabled
    DPKG_DEB_COMPRESSOR_THREADS="$threads" dpkg-deb --root-owner-group "-Z$compressor" "-z$level" -b "$WORK_DIR" "$deb_filename"
    
    if [ $? -eq 0 ]; then
        log_info "DEB package built successfully: $deb_filename"
        
        # Display package information
        echo
        log_info "Package information:"
        dpkg-deb -I "$deb_filename"
        
        echo
        log_info "Package contents:"
        dpkg-deb -c "$deb_filename"
        
        echo
        log_info "Package file: $(pwd)/$deb_filename"
        log_info "Package size: $(du -h "$deb_filename" | cut -f1)"
    else
        error_exit "DEB package build failed"
    fi
}

# Main function
main() {
    # Execute different cargo commands based on build mode
    if [ "$BUILD_MODE" = "release" ]; then
        log_info "Executing release build..."
        VCPKG_ROOT=$HOME/vcpkg cargo build --release --features cli
    else
        log_info "Executing debug build..."
        VCPKG_ROOT=$HOME/vcpkg cargo build --features cli
    fi
    
    log_info "Starting to build RustDesk DEB package..."
    log_info "Build mode: $BUILD_MODE"
    log_info "Version: $VERSION"
    log_info "Architecture: $DEB_ARCH"
    
    # Display binary file size
    if [ -f "$BINARY_PATH" ]; then
        local file_size_mb
        file_size_mb=$(du -m "$BINARY_PATH" | cut -f1)
        log_info "Binary file size: ${file_size_mb}MB"
    fi
    
    check_dependencies
    create_directory_structure
    copy_files
    generate_control_file
    copy_debian_scripts
    generate_md5sums
    build_deb_package
    
    log_info "DEB package build completed!"
}

# Show help information
show_help() {
    echo "Usage: $0 [options]"
    echo ""
    echo "Options:"
    echo "  -h, --help     Show this help information"
    echo "  -v, --version  Specify version number (default: $VERSION)"
    echo "  -a, --arch     Specify architecture (default: $DEB_ARCH)"
    echo "  -m, --mode     Specify build mode (debug|release, default: $BUILD_MODE)"
    echo ""
    echo "Environment variables:"
    echo "  BUILD_MODE     Set build mode (debug, release)"
    echo "  DEB_ARCH       Set target architecture (amd64, arm64, armhf, riscv64)"
    echo ""
    echo "Examples:"
    echo "  $0                          # Use default settings to build (debug mode))"
    echo "  $0 -m release               # Build release version"
    echo "  $0 -v 1.4.3 -m release     # Specify version and release mode"
    echo "  $0 -a arm64 -m release      # Specify architecture and release mode"
    echo "  BUILD_MODE=release $0       # Specify release mode through environment variable"
}

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        -h|--help)
            show_help
            exit 0
            ;;
        -v|--version)
            VERSION="$2"
            shift 2
            ;;
        -a|--arch)
            DEB_ARCH="$2"
            shift 2
            ;;
        -m|--mode)
            if [ "$2" != "debug" ] && [ "$2" != "release" ]; then
                log_error "Build mode must be "debug" or "release"'"
                exit 1
            fi
            BUILD_MODE="$2"
            # Reset binary file path
            if [ "$BUILD_MODE" = "release" ]; then
                BINARY_PATH="target/release/rustdesk"
            else
                BINARY_PATH="target/debug/rustdesk"
            fi
            shift 2
            ;;
        *)
            log_error "Unknown option: $1"
            show_help
            exit 1
            ;;
    esac
done

# Run main function
main
