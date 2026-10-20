# Offline launcher

Tiny static server: serves the `game` folder next to the binary on a free local port and opens
the default browser. Used for the downloadable zips (owner request 2026-10-20).

Build (no cgo, cross-compiles from Linux):

    cd tools/launcher
    CGO_ENABLED=0 GOOS=windows GOARCH=amd64 go build -ldflags "-s -w" -o out/WreckPursuit.exe .
    CGO_ENABLED=0 GOOS=darwin  GOARCH=arm64 go build -ldflags "-s -w" -o out/WreckPursuit-mac-apple .
    CGO_ENABLED=0 GOOS=darwin  GOARCH=amd64 go build -ldflags "-s -w" -o out/WreckPursuit-mac-intel .
    CGO_ENABLED=0 GOOS=linux   GOARCH=amd64 go build -ldflags "-s -w" -o out/WreckPursuit-linux .

Zip layout: `WreckPursuit-<Kind>/{WreckPursuit.exe, WreckPursuit-mac-*, WreckPursuit-linux,
README.txt, game/<the published build>}`. The binaries are not code-signed (SmartScreen /
Gatekeeper warn on first run).
