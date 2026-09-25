# Aquarium — common tasks (https://github.com/casey/just).
# Without `just`, each recipe is the command it shows.

default: run

# Download the CC0 assets (once, ~61 MB)
fetch-assets:
    ./scripts/fetch_assets.sh

# Full screen, high quality
run: fetch-assets
    cargo run --release

# 1600x1000 window
windowed: fetch-assets
    cargo run --release -- --windowed

# Power saving: 30 fps, half the particles
low-power: fetch-assets
    cargo run --release -- --low-power

# Live wallpaper (below the desktop icons), in the background: it sleeps in
# slow motion and wakes up when the mouse crosses the desktop (or ⌃⌥⌘A)
wallpaper: fetch-assets
    cargo build --release
    nohup ./target/release/aquarium --wallpaper >/dev/null 2>&1 &

# Stop the wallpaper
stop-wallpaper:
    -pkill -x aquarium

# Start the wallpaper at every login (user LaunchAgent)
install-login: fetch-assets
    cargo build --release
    mkdir -p ~/Library/LaunchAgents
    printf '%s\n' '<?xml version="1.0" encoding="UTF-8"?>' \
      '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">' \
      '<plist version="1.0"><dict>' \
      '<key>Label</key><string>local.aquarium.wallpaper</string>' \
      '<key>ProgramArguments</key><array><string>{{justfile_directory()}}/target/release/aquarium</string><string>--wallpaper</string></array>' \
      '<key>EnvironmentVariables</key><dict><key>AQUARIUM_ASSETS</key><string>{{justfile_directory()}}/assets</string></dict>' \
      '<key>RunAtLoad</key><true/>' \
      '</dict></plist>' > ~/Library/LaunchAgents/local.aquarium.wallpaper.plist
    launchctl load ~/Library/LaunchAgents/local.aquarium.wallpaper.plist

# Remove the login item
uninstall-login:
    -launchctl unload ~/Library/LaunchAgents/local.aquarium.wallpaper.plist
    rm -f ~/Library/LaunchAgents/local.aquarium.wallpaper.plist

# Optimized binary (fat LTO): target/dist/aquarium
dist: fetch-assets
    cargo build --profile dist

# Offscreen reference renders in docs/shots/ (one per camera preset)
shots: fetch-assets
    cargo build --release
    mkdir -p docs/shots
    for v in fill front hero side top low close inside; do ./target/release/aquarium --view $v --screenshot docs/shots/$v.png --frames 300; done

# 4K (3840x2160) capture of the full-screen view
shot-4k: fetch-assets
    cargo build --release
    AQ_RES=3840x2160 ./target/release/aquarium --screenshot docs/aquarium-4k.png --frames 400

# Physics test bench: 2 simulated minutes, 81 checks on every animal
# (AQ_PHYSICS_DT=30 or vsync for 30 fps or irregular frame times)
physics: fetch-assets
    cargo build --release
    mkdir -p out/physics
    AQ_PHYSICS=out/physics AQ_AUTOPILOT=1 AQ_RES=320x208 ./target/release/aquarium --screenshot out/physics/last.png --frames 7200
    python3 scripts/physics_report.py out/physics
