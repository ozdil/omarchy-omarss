# OmaRSS (`ozdil.omarss`)

Ultra-fast, native, lightweight RSS & Atom feed reader and notification hub for Omarchy Linux written in Rust.

## Features
- **100% Native Rust Engine (`omarss-engine`)**: Zero shell string concatenation, bounded process-group `curl` fetching with POSIX timeouts, monotonic deadlines, and memory caps.
- **Universal XML Feed Parser**: Parses RSS 2.0 and Atom feeds with HTML entity decoding and tag stripping.
- **Dynamic Alert Coloring**: RSS icon (``) illuminates green (`#22c55e`) when new unread articles exist, and seamlessly fades to standard theme color when all articles are caught up.
- **Native Omarchy UI (`Panel.qml`)**:
  - Filter tabs: `Unread (N)`, `All (N)`, `Feeds (N)`.
  - Rich article cards with feed badge, publication date, title, excerpt, and unread pill.
  - 1-click browser opener (`xdg-open`) with automatic mark-read.
  - Interactive feed manager: Add RSS/Atom feeds, toggle feeds on/off, or unsubscribe.
  - Quick actions: Mark all read (`✓`), refresh feeds (``).
- **Standard Arch Linux PKGBUILD** included.

## CLI Usage
```bash
omarss-engine --status        # Clean ASCII summary table
omarss-engine --json          # Machine-readable JSON output for Quickshell
omarss-engine --refresh       # Fetch all active feeds in parallel
omarss-engine --mark-read <id># Mark article as read
omarss-engine --mark-all-read # Mark all articles as read
omarss-engine --add-feed <url> [name] # Subscribe to a new feed
omarss-engine --remove-feed <url>     # Unsubscribe
omarss-engine --toggle-feed <url>     # Enable / disable feed
omarss-engine --open-url <url> [id]   # Open in browser and mark read
```

## License
MIT
